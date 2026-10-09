# Memechan 文字检索模型对比

评测日期：2026-10-09。建议当前优先接入 **Qwen3-Embedding-0.6B**：所测包能在应用现有 ONNX Runtime 1.22 上推理，查询延迟最低。Harrier 270M 是值得保留的低内存候选；这次小样本不足以断言任何一个模型的检索质量普遍更好。

本次只增加离线评测工具、固定样本和结果，没有切换应用中的 BGE 模型，也没有修改应用依赖。

## 本地结果

62 条合成表情包描述、74 条查询，覆盖动作、情绪、口语意图、易混淆动作、跨语言和模拟 OCR 噪声。样本及相关性标注在获得模型分数前固定，三者使用相同样本和英文任务指令。它们不是用户真实图片，也不是实际图片理解 API 的输出。

Ryzen 7 7735H、27 GiB 内存、Linux，CPU 推理；统一 ONNX Runtime 1.30.0，两个计算线程、一个跨算子线程，batch=1。测试社区提供的 `model_quantized.onnx` 量化包；量化方法、图优化和是否内置池化并不完全一致，下面比较的是这些可部署文件，不能把速度差异完全归因于模型架构。

| 指标                        |   Harrier 270M |   Harrier 0.6B |     Qwen3 0.6B |
| --------------------------- | -------------: | -------------: | -------------: |
| 首位命中                    | 54/74（73.0%） | 52/74（70.3%） | 50/74（67.6%） |
| 前五项至少命中一个          | 66/74（89.2%） | 69/74（93.2%） | 66/74（89.2%） |
| nDCG@5                      |         73.84% |         76.06% |         74.32% |
| 动作查询首位命中            |          25/30 |          25/30 |          24/30 |
| 查询编码耗时 p50            |         136 ms |         863 ms |          89 ms |
| 查询编码耗时 p95            |         149 ms |         888 ms |          99 ms |
| 62 条描述编码总耗时         |         8.39 s |        55.03 s |         4.92 s |
| 评测进程峰值 RSS            |        490 MiB |        702 MiB |        912 MiB |
| ONNX 权重体积（含外部数据） |        328 MiB |        674 MiB |        585 MiB |
| 向量维度                    |            640 |           1024 |           1024 |

“命中”包含直接相关和可接受的次要结果；nDCG 使用标注等级 2/1/0，增益为 `2^grade - 1`。相关性由单一标注者编写，未做独立人工复核。完整逐查询排名、分组指标和耗时见 [results](results/)；可用 [summarize.mjs](summarize.mjs) 重新计算。

| 分组首位命中  | Harrier 270M | Harrier 0.6B | Qwen3 0.6B |
| ------------- | -----------: | -----------: | ---------: |
| 动作（30）    |           25 |           25 |         24 |
| 情绪（10）    |            6 |            3 |          4 |
| 口语意图（8） |            5 |            4 |          7 |
| 易混淆（8）   |            4 |            4 |          4 |
| 跨语言（10）  |            8 |            9 |          6 |
| OCR 噪声（8） |            6 |            7 |          5 |

用户关心的短动作查询尤其值得单独看：

| 查询           | Harrier 270M        | Harrier 0.6B | Qwen3 0.6B   |
| -------------- | ------------------- | ------------ | ------------ |
| 敬礼           | 首个相关结果第 5 名 | 第 1 名      | 第 1 名      |
| 打滚           | 前五项未命中        | 前五项未命中 | 第 1 名      |
| 哭             | 第 1 名             | 第 1 名      | 前五项未命中 |
| 笑到满地打滚   | 前五项未命中        | 前五项未命中 | 第 1 名      |
| 委屈但是强忍着 | 前五项未命中        | 第 1 名      | 第 1 名      |

例如“打滚”的目标描述包括身体在地板上来回翻动的中文描述，以及英文和日文描述；并非只测字面包含查询词的情况。这轮 Qwen 在口语意图方面表现较好，Harrier 在跨语言方面较好，但每组只有 8～30 条查询，不能推广成稳定的能力排名。

## MTEB 与过拟合疑问

从 [MTEB Multilingual v2 官方榜单](https://leaderboard.mteb.org/benchmark/MTEB%28Multilingual%2C%20v2%29)的接口读取同一版本、同一组 131 个任务，保留了三者的逐任务分数和训练任务标记：[mteb-snapshot.json](mteb-snapshot.json)。

| 指标                            | Harrier 270M | Harrier 0.6B | Qwen3 0.6B |
| ------------------------------- | -----------: | -----------: | ---------: |
| Mean Task                       |        66.55 |        69.01 |      64.34 |
| Retrieval                       |        66.38 |        70.75 |      64.65 |
| 榜单标记零样本任务占比          |          78% |          78% |        99% |
| 共同未标记训练的 103 个任务均分 |        65.77 |        67.66 |      65.26 |

最后一行是本次重新计算的结果，不是榜单原始总分：取三个模型 `trainedOnTasks` 的并集，从任务集合中剔除，再对三者都有结果的 103 个任务等权求均值。Harrier 相对 Qwen 的差距从约 2.21/4.68 分缩小到约 0.51/2.40 分。

Harrier 的训练任务覆盖更多，因此直接把总分差当作陌生任务上的优势不够稳妥。但“训练过同类任务或该数据集的训练划分”不等于“使用测试集训练”。这些标记不能证明测试集泄漏或过拟合；共同未标记训练的任务也不能保证训练语料完全无交叉。本次合成样本仅用于检验 Memechan 场景的初步适配，无法解决训练污染审计问题。

## 接入成本

三者都能使用现有 Rust + ONNX 路线，不需要 Python 或第二套推理框架。它们的官方用法均要求查询指令、最后一个有效 token 的池化和 L2 归一化；所测 Harrier 导出图已经包含池化，直接返回 `sentence_embedding`。

- **共同修改**：现有应用写死 BGE 的 CLS 池化、512 维、查询前缀和 150 MiB 文件上限，需要改为对应模型的输入输出约定和文件清单。更换模型必须重建向量索引，即使向量维度相同也不能混用。
- **Qwen**：本次导出包含 `position_ids` 和 KV 缓存输入，独立文本需传入长度为零的缓存。评测工具已经实现，并在应用现有 ORT 1.22 上实际执行了官方模型卡的两条查询，相关文档均排第一；这不是与原始全精度模型数值一致的验证。
- **Harrier**：两个所测量化包在 ORT 1.22 上均因 `GatherBlockQuantized` 的 `bits` 属性不受支持而加载失败。通过 `ort/load-dynamic` 加载独立 ORT 1.30.0 后均成功，无需升级 Rust `ort` crate。但正式接入还需要较新运行库的跨平台打包及 OCR 兼容性验证；也可以另找或重新导出兼容旧运行库的模型。本次没有验证最早支持它们的 ORT 版本。
- **Harrier 外部权重文件**：模型图和 `.onnx_data` 必须一起导入，保留文件对应关系；Qwen 此包为单文件权重。

因此当前选型偏向 Qwen 是延迟、现有运行库兼容性和维护成本的取舍，不能据此宣称它检索质量胜过 Harrier。Harrier 270M 的内存优势明显；Harrier 0.6B 在本轮多命中 3 条前五项结果，但所测包的查询延迟约为 Qwen 的 9.7 倍，暂不作为默认选择。其他导出或量化方案的表现需要另测。

模型官方说明：[Harrier 270M](https://huggingface.co/microsoft/harrier-oss-v1-270m)、[Harrier 0.6B](https://huggingface.co/microsoft/harrier-oss-v1-0.6b)、[Qwen3 0.6B](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B)。

## 复现与限制

[models.json](models.json) 固定了社区导出仓库 revision、文件 SHA-256、运行库来源、机器参数和语料校验和。权重不加入项目，当前下载在 `/tmp/memechan-embedding-eval/models/`。评测不联网，也不读取应用表情库；下载模型是独立的准备步骤。

在项目根目录运行，以下以 Harrier 270M 为例，替换模型目录即可运行另外两者：

```bash
ORT_DYLIB_PATH=/tmp/memechan-embedding-eval/onnxruntime-linux-x64-1.30.0/lib/libonnxruntime.so.1.30.0 \
  CARGO_BUILD_JOBS=2 cargo run --offline --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features ort/load-dynamic --example embedding_bench -- \
  /tmp/memechan-embedding-eval/models/harrier-270m \
  benchmarks/embeddings/meme-retrieval.json \
  /tmp/memechan-embedding-eval/harrier-270m.json

node benchmarks/embeddings/summarize.mjs benchmarks/embeddings/results/harrier-270m.json \
  benchmarks/embeddings/results/harrier-0.6b.json benchmarks/embeddings/results/qwen-0.6b.json
```

- 每个模型单独进程、串行测试，先预热三个固定句子，再编码文档和查询；最大长度 512 token，本次样本未达到上限。
- 耗时包含分词、输入构造、推理和归一化，不包含模型初次加载、向量搜索、UI、OCR 或远程图片描述。每条查询测一次，p50/p95 是不同查询的耗时分布，不是同一请求反复测量。
- Rust 评测程序使用 dev 构建，ORT 是预编译 Release 库。未测试应用 Release 包、GPU、Windows/macOS、不同线程数、长描述或大型库。运行时内存是评测进程 RSS，不代表整个桌面应用内存。
- 仅评测社区量化文件，未与官方全精度权重做向量数值对齐。三者的量化配方和导出图不同，模型本身、量化误差及导出优化的影响没有分离。
- 样本较小且由助手编写，未根据运行结果修改标注或调参；不能将 2～4 条命中差异当成确定胜负。后续真实图片描述分布可能改变结果。
- 动作必须先出现在自定义描述或图片理解 API 的描述中。OCR 只能提供图中文字，这轮没有验证从无字图片识别“敬礼、打滚、哭”的能力。
