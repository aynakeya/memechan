"""Export local Qwen3-Embedding weights, or package an existing ONNX export for Memechan."""

import argparse
import hashlib
import json
import os
import re
import shutil
from pathlib import Path

# This tool only consumes local weights. Installing its dependencies is a separate step.
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"

DIMENSIONS = {"0.6b": 1024, "4b": 2560, "8b": 4096}


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def tensors(graph):
    import onnx

    yield from graph.initializer
    for node in graph.node:
        for attribute in node.attribute:
            if attribute.type == onnx.AttributeProto.TENSOR:
                yield attribute.t
            elif attribute.type == onnx.AttributeProto.TENSORS:
                yield from attribute.tensors
            elif attribute.type == onnx.AttributeProto.GRAPH:
                yield from tensors(attribute.g)
            elif attribute.type == onnx.AttributeProto.GRAPHS:
                for child in attribute.graphs:
                    yield from tensors(child)


def package(onnx_path, tokenizer, size):
    import onnx

    model = onnx.load(str(onnx_path), load_external_data=False)
    inputs = {item.name for item in model.graph.input}
    if not {"input_ids", "attention_mask"} <= inputs:
        raise ValueError("The model must expose input_ids and attention_mask.")
    if any(
        name not in {"input_ids", "attention_mask", "position_ids"}
        and not name.startswith("past_key_values.")
        for name in inputs
    ):
        raise ValueError(f"Unsupported model inputs: {sorted(inputs)}")
    output = next(
        (
            item
            for item in model.graph.output
            if item.name in {"last_hidden_state", "token_embeddings"}
        ),
        None,
    )
    if output is None:
        raise ValueError(
            "Export feature-extraction, with a last_hidden_state or token_embeddings output."
        )
    tensor = output.type.tensor_type
    if (
        tensor.elem_type != onnx.TensorProto.FLOAT
        or len(tensor.shape.dim) != 3
        or tensor.shape.dim[-1].dim_value != DIMENSIONS[size]
    ):
        raise ValueError(
            f"Expected float32 token embeddings with {DIMENSIONS[size]} dimensions."
        )

    files = {onnx_path.name, "tokenizer.json"}
    for tensor in tensors(model.graph):
        if tensor.data_location == onnx.TensorProto.EXTERNAL:
            files.update(
                item.value for item in tensor.external_data if item.key == "location"
            )
    for name in files:
        if not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", name):
            raise ValueError(f"Use flat filenames for external weights: {name}")
        if name != "tokenizer.json" and not (onnx_path.parent / name).is_file():
            raise FileNotFoundError(f"Missing referenced weight file: {name}")
    destination = onnx_path.parent / "tokenizer.json"
    if tokenizer.resolve() != destination.resolve():
        shutil.copy2(tokenizer, destination)
    # ONNX Runtime validates operators when loading. Its optimized operators are
    # not all registered with the generic ONNX schema checker.
    manifest = {
        "model": f"qwen3-{size}",
        "onnx": onnx_path.name,
        "tokenizer": "tokenizer.json",
        "output": output.name,
        "files": {name: digest(onnx_path.parent / name) for name in sorted(files)},
    }
    path = onnx_path.parent / "memechan-model.json"
    path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"Import this file in Memechan: {path}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument(
        "--source",
        type=Path,
        help="Local directory containing official Qwen3-Embedding weights",
    )
    source.add_argument(
        "--onnx",
        type=Path,
        help="Package an existing ONNX graph without exporting again",
    )
    parser.add_argument("--size", choices=DIMENSIONS, required=True)
    parser.add_argument("--output", type=Path, help="New output directory for --source")
    parser.add_argument(
        "--tokenizer", type=Path, help="Matching tokenizer.json; required with --onnx"
    )
    parser.add_argument(
        "--precision",
        choices=("int8", "fp32"),
        default="fp32",
        help="FP32 preserves embeddings; experimental INT8 requires retrieval quality validation",
    )
    args = parser.parse_args()
    if args.onnx:
        if not args.tokenizer:
            parser.error("--onnx requires --tokenizer")
        package(args.onnx.resolve(), args.tokenizer, args.size)
        return
    if not args.source.is_dir() or not args.output:
        parser.error("--source requires a local model directory and --output")
    if args.output.exists() and any(args.output.iterdir()):
        parser.error("--output must be empty; existing exports are never overwritten")
    config = json.loads((args.source / "config.json").read_text())
    if (
        config.get("model_type") != "qwen3"
        or config.get("hidden_size") != DIMENSIONS[args.size]
    ):
        parser.error(
            "The local model configuration does not match the selected Qwen size"
        )

    import onnxruntime
    from optimum.exporters.onnx import main_export

    onnxruntime.disable_telemetry_events()

    raw = args.output / "fp32" if args.precision == "int8" else args.output
    main_export(
        model_name_or_path=str(args.source.resolve()),
        output=raw,
        task="feature-extraction",
        library_name="transformers",
        local_files_only=True,
        trust_remote_code=False,
        dtype="fp32",
        opset=18,
        batch_size=1,
        sequence_length=32,
        do_validation=True,
    )
    graph = raw / "model.onnx"
    if args.precision == "int8":
        from onnxruntime.quantization import QuantType, quantize_dynamic

        print(
            "Experimental INT8: 0.6B testing found substantial embedding drift; "
            "validate retrieval quality before replacing FP32.",
            flush=True,
        )
        graph = args.output / "model_int8.onnx"
        quantize_dynamic(
            model_input=str(raw / "model.onnx"),
            model_output=str(graph),
            weight_type=QuantType.QUInt8,
            per_channel=True,
            reduce_range=True,
            use_external_data_format=True,
            op_types_to_quantize=["MatMul", "Gather"],
        )
    package(graph.resolve(), args.source / "tokenizer.json", args.size)


if __name__ == "__main__":
    main()
