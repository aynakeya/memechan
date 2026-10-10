//! Offline comparison of exported text embedding models; does not open the app library.
use memechan_core::Result;
use ort::{session::Session, value::Tensor};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, env, fs, path::Path, time::Instant};
use tokenizers::{Tokenizer, TruncationParams};

#[derive(Deserialize)]
struct Corpus {
    instruction: String,
    documents: Vec<Document>,
    queries: Vec<Query>,
}

#[derive(Deserialize)]
struct Document {
    id: String,
    text: String,
}

#[derive(Deserialize, Serialize)]
struct Query {
    category: String,
    text: String,
    relevant: BTreeMap<String, u8>,
}

fn embed(session: &mut Session, tokenizer: &Tokenizer, text: &str) -> Result<(Vec<f32>, usize)> {
    let encoding = tokenizer.encode(text, true)?;
    let length = encoding.len();
    let mut inputs = Vec::new();
    for input in session.inputs() {
        // This Qwen export includes decoder caches; independent texts start with empty caches.
        if input.name().starts_with("past_key_values.") {
            let shape = input
                .dtype()
                .tensor_shape()
                .ok_or("Expected a tensor cache input")?;
            if shape.len() != 4 || shape[1] <= 0 || shape[3] <= 0 {
                return Err(format!("Unsupported cache shape: {shape:?}").into());
            }
            inputs.push((
                input.name().to_owned(),
                Tensor::<f32>::new(session.allocator(), [1, shape[1], 0, shape[3]])?.into_dyn(),
            ));
            continue;
        }
        let values: Vec<i64> = match input.name() {
            "input_ids" => encoding.get_ids().iter().map(|&n| i64::from(n)).collect(),
            "attention_mask" => encoding
                .get_attention_mask()
                .iter()
                .map(|&n| i64::from(n))
                .collect(),
            "position_ids" => (0..length as i64).collect(),
            other => return Err(format!("Unsupported graph input: {other}").into()),
        };
        inputs.push((
            input.name().to_owned(),
            Tensor::from_array(([1, length], values))?.into_dyn(),
        ));
    }
    let outputs = session.run(inputs)?;
    let mut vector = if let Some(output) = outputs.get("sentence_embedding") {
        let (shape, values) = output.try_extract_tensor::<f32>()?;
        if shape.len() != 2 || shape[0] != 1 {
            return Err(format!("Unexpected sentence embedding shape: {shape:?}").into());
        }
        values.to_vec()
    } else if let Some(output) = outputs.get("last_hidden_state") {
        let (shape, values) = output.try_extract_tensor::<f32>()?;
        if shape.len() != 3 || shape[0] != 1 || shape[1] != length as i64 {
            return Err(format!("Unexpected hidden state shape: {shape:?}").into());
        }
        let dimensions = shape[2] as usize;
        values[(length - 1) * dimensions..length * dimensions].to_vec()
    } else {
        return Err("Expected sentence_embedding or last_hidden_state output".into());
    };
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err("Model produced invalid embeddings".into());
    }
    for value in &mut vector {
        *value /= norm;
    }
    Ok((vector, length))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err("Usage: embedding_bench MODEL_DIRECTORY CORPUS_JSON OUTPUT_JSON".into());
    }
    let directory = Path::new(&args[1]);
    let corpus: Corpus = serde_json::from_slice(&fs::read(&args[2])?)?;
    ort::init().with_telemetry(false).commit();
    let start = Instant::now();
    let mut session = Session::builder()?
        .with_intra_threads(2)
        .map_err(ort::Error::<()>::from)?
        .with_inter_threads(1)
        .map_err(ort::Error::<()>::from)?
        .commit_from_file(directory.join("onnx/model_quantized.onnx"))?;
    let mut tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))?;
    tokenizer.with_padding(None);
    tokenizer.with_truncation(Some(TruncationParams {
        max_length: 512,
        ..Default::default()
    }))?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "Loaded in {load_ms:.0} ms; {} inputs, {} outputs; {}",
        session.inputs().len(),
        session.outputs().len(),
        ort::info()
    );

    // Identical unscored warm-up texts across models.
    for text in [
        "这是用于预热的短句。",
        "A short warm-up sentence.",
        "你好，世界。",
    ] {
        embed(&mut session, &tokenizer, text)?;
    }
    let mut documents = Vec::new();
    let mut document_times = Vec::new();
    let mut document_tokens = Vec::new();
    for document in &corpus.documents {
        let start = Instant::now();
        let (vector, tokens) = embed(&mut session, &tokenizer, &document.text)?;
        document_times.push(start.elapsed().as_secs_f64() * 1000.0);
        document_tokens.push(tokens);
        documents.push(vector);
    }
    eprintln!("Indexed {} documents", documents.len());
    let mut results = Vec::new();
    for (index, query) in corpus.queries.iter().enumerate() {
        let text = format!("Instruct: {}\nQuery: {}", corpus.instruction, query.text);
        let start = Instant::now();
        let (vector, tokens) = embed(&mut session, &tokenizer, &text)?;
        let encode_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut ranked: Vec<_> = documents
            .iter()
            .enumerate()
            .map(|(i, document)| {
                let score: f32 = vector.iter().zip(document).map(|(a, b)| a * b).sum();
                (i, score)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        results.push(json!({
            "query": query,
            "encode_ms": encode_ms,
            "tokens": tokens,
            "top10": ranked.iter().take(10).map(|(i, score)| json!({
                "id": corpus.documents[*i].id,
                "score": score,
            })).collect::<Vec<_>>()
        }));
        if (index + 1) % 10 == 0 {
            eprintln!("Evaluated {}/{} queries", index + 1, corpus.queries.len());
        }
    }
    let status = fs::read_to_string("/proc/self/status")?;
    let memory = status
        .lines()
        .filter(|line| line.starts_with("VmHWM:") || line.starts_with("VmRSS:"))
        .collect::<Vec<_>>();
    let output = json!({
        "model_directory": directory,
        "runtime": ort::info(),
        "threads": 2,
        "max_tokens": 512,
        "load_ms": load_ms,
        "dimension": documents[0].len(),
        "document_encode_ms": document_times,
        "document_tokens": document_tokens,
        "linux_memory": memory,
        "results": results,
    });
    fs::write(&args[3], serde_json::to_vec_pretty(&output)?)?;
    eprintln!("Saved {}", args[3]);
    Ok(())
}
