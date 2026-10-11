# Memechan · 表情资料库

一个使用 **Tauri 2 + Vue 3 + Nuxt UI 4 + TypeScript + Vite + Rust + SQLite** 的本地桌面表情库。没有 Electron，也不需要 Python 服务。

界面使用 Nuxt UI 的 Vue/Vite 模式，统一按钮、表单、菜单、弹窗和通知，不需要 Nuxt 服务端。主题在 `vite.config.ts` 和 `src/style.css` 中配置；使用系统字体，Lucide 图标在构建时从本地依赖打包，运行时不请求字体或图标服务。

## 当前实现

- 拖入或选择 PNG、JPEG、GIF、WebP / APNG，复制原文件到资料库，按 SHA-256 去重。
- 分组、收藏、最近添加、未分组、动图筛选、原图预览，以及自定义描述。
- 双击复制，右键选择原尺寸、128 / 256 / 512 px 或自定义最长边；默认尺寸可设置，保持比例，可选择是否放大小图。
- 静图写入图片和文件剪贴板格式。动图写入原始文件与对应格式，不以静态首帧代替动画。
- GIF 缩放逐帧处理；动态 WebP / APNG 缩放后转换成 GIF。原文件始终保留。
- 可开关的本地 PP-OCRv5 mobile 中文、英文和日文识别；动图在前 180 帧中最多抽样 5 帧并合并识别文字。
- Qwen3-Embedding 0.6B / 4B / 8B 多语言文本向量检索：文件名、自定义描述、OCR 文字和 API 图片描述作为检索内容。使用查询指令、末个有效 token 的 1024 / 2560 / 4096 维向量、L2 归一化和余弦相似度；智能搜索通过倒数排名融合关键词与向量结果。
- 可配置 OpenAI 兼容的图片理解 API，支持单张生成、批量队列、暂停和可选的导入后自动生成。描述独立保存，不覆盖手写内容。
- 模型仅从本地文件导入；预置 OCR / 0.6B 使用固定 SHA-256，自导出模型按文件清单校验；本地保存索引，可暂停任务、重试失败项目。
- 任务管理集中显示 OCR、图片理解和文字索引的进度，分别暂停、继续及重试失败项。OCR 文字和 API 图片描述可逐张或全库清除，保留手写描述与原图。

未配置图片理解 API 时，没有文字且没有自定义描述的图片只能依赖文件名检索。云同步和直接用图片向量检索暂未实现。

## 启动

使用 pnpm 11.12.0 管理依赖，版本固定在 `package.json` 的 `packageManager`。需要 Node.js 22.13+（推荐 24）、Rust 1.90+、C/C++ 构建工具和各平台的 [Tauri 开发依赖](https://v2.tauri.app/start/prerequisites/)。

Ubuntu / Debian 先安装开发包：

```bash
sudo apt install build-essential cmake pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev patchelf
```

然后：

```bash
git submodule update --init --recursive
pnpm install
pnpm tauri dev
```

`vendor/paddle-ocr-rs` 是指向 [OCR fork](https://github.com/aynakeya/paddle-ocr-rs) 的 Git submodule，Cargo 通过本地路径引用其 `rapid-ocr-rs` crate。主仓库固定子模块提交，不自动跟随分支。修改 OCR 库时先在子模块中提交并推送，再在主仓库提交子模块指针；上游合并适配后，可用 `git submodule set-url vendor/paddle-ocr-rs <上游地址>` 切换来源，并在子模块中检出已合并的提交。

`pnpm dev` 只打开浏览器界面预览。浏览器中明确禁用本地文件、剪贴板和模型功能，真实功能需要 Tauri 桌面窗口。第一次编译需下载 Rust 依赖和 ONNX Runtime。

日常使用或评估性能时，可用 `pnpm tauri dev --release` 运行优化后的后端。普通开发模式也为图片处理、编解码和 SHA-256 依赖开启了优化，应用代码保留调试配置；首次编译这些依赖会更久，之后可复用编译缓存。

在对应平台打包：

```bash
pnpm tauri build
```

跨平台代码和图标已准备，当前验证了 Linux 开发版本。Windows/macOS 的构建、签名和粘贴兼容性仍需在对应环境验证。ONNX Runtime 由 `ort` 按目标平台链接；发布安装包时还需核对动态运行库是否完整分发。

## 使用

把图片拖入窗口即可添加；打开分组后导入，会加入该分组。单击选中，工具栏「详情」开关查看描述、OCR 和所属分组；双击复制，空格预览，Ctrl / Cmd K 搜索。描述可以显式保存，也会在切换所选图片时保存。

网格按可见行虚拟渲染，支持窗口缩放和紧凑模式。后台轮询只在资料库数据变化后刷新卡片，单纯的任务进度变化不再重新加载资料库。当前元数据仍通过完整快照读取，尚未做数据库分页。

搜索保留 350 ms 防抖，中文组词期间不发起查询；输入、切换搜索条件或清空时立即取消尚未开始的旧搜索。交互搜索优先于下一张后台 OCR / 索引任务，已开始的推理允许完成，过期结果不再使用。后台刷新不会重发正在执行的查询；关键词搜索无需等待推理队列。

在「偏好设置 → OCR / Embedding」点击「导入本地模型」，可多选下表中的 ONNX 和 tokenizer 文件，自动识别内容并复制到资料库，无需重命名，不移动原文件。支持分批导入并校验，界面会列出缺少的文件。OCR 和文字索引独立加载模型；在 OCR 页签关闭「启用本地 OCR」后，只需导入 Qwen 模型即可使用文字语义检索，也不会加载 OCR 模型。之后导入图片、修改描述会自动在后台更新索引；在 Embedding 页签关闭自动索引后，可以手动处理。已完成索引的图片立即参与语义搜索，无需等待整批完成；模型未安装时智能搜索使用关键词匹配，纯语义搜索会明确提示。

大量导入时，每张图片独立保存并加入待处理队列，导入期间会更新资料库和进度。本地推理一次只处理一张图片，逐张保存 OCR 和向量，失败项不会阻塞后续图片。侧栏「任务管理」分别显示全库已识别 / 未识别、已生成 / 未生成、已索引 / 未索引的数量，未完成数量包含失败项；三类任务使用统一的总进度条，暂停和完成后保留；仅运行时显示当前图片，完成后隐藏继续操作。统计读取已保存的图片状态，重启后保留，清除内容后同步更新。暂停在当前图片处理完成后生效，新增图片不会自动解除暂停。「继续」处理未完成项，「重试失败项」将失败图片重新入队。暂停状态保留到本次应用退出；重启时，已启用且模型就绪的本地任务会继续 SQLite 中的未完成项。尚未保存的导入文件需要重新选择；已导入文件会去重。

图片详情中的清除按钮只作用于当前图片；任务管理中的清除按钮作用于整个资料库，均需确认。清除 OCR 或 API 描述会同时移除旧向量并排队更新索引，不影响手写描述、原图、分组和收藏；正在生成的对应结果也不会填回。清除内容不会因继续任务、失败重试或重启而自动生成；需要时可主动重新识别或生成缺失描述。任务管理还可清除全部文字向量索引，保留原图与所有文字；清除后的图片暂停自动索引，OCR / API 返回结果、编辑描述及重启都不会自动重建。点击「建立缺失索引」后恢复这些图片的索引；新导入的图片仍遵循自动索引设置。

设置的尺寸表示图片最长边；填 `0` 使用原尺寸。动图转换为 GIF 会受 GIF 色彩、透明度和时间精度限制。首次缩放可能需要等待，再次复制使用缓存。

Linux / macOS 复制静态 PNG 时直接使用 PNG 数据，避免重复解码和编码；有 EXIF 旋转信息的图片会先校正方向。JPG、WebP 和需要旋转的 PNG，其剪贴板 PNG 转换结果保存在 `copies/`，重复复制同一图片和尺寸直接复用，并与其他复制副本共用 512 MiB 软上限及清理规则。Windows 继续使用原生位图剪贴板格式。

**动图粘贴取决于接收方。** Discord、Telegram 桌面版、各浏览器及具体网页对文件剪贴板的支持不同；不能保证任意网页编辑框都接受 GIF。Linux 上还取决于 X11 / Wayland 与桌面剪贴板协议。当前实现写入动图文件，不静默降级为静图，需要在实际目标软件里检查发送后的动画。

## 离线模型

本地文件导入、模型加载、OCR 和文字搜索不需要外部请求，ONNX Runtime 的遥测在初始化时显式关闭。应用不提供模型联网下载；模型文件需自行准备后导入。本地推理不会上传图片，配置并主动使用下述图片理解 API 时才会发送图片。推理在本地 CPU 执行，每个会话限制为 2 个线程，图片逐张处理。

模型目录在偏好设置中显示。OCR 导入下表的 ONNX 文件；所有 Qwen 向量模型统一导入 `memechan-model.json` 模型包。

下表的 OCR 文件在导入时校验固定 SHA-256，无需重命名。若直接复制到应用模型目录，则需按左栏重命名并重启应用。

| 本地文件名        | 固定版本下载地址                                                                                                                                                    |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ocr-v5-det.onnx` | [PP-OCRv5 mobile detection](https://www.modelscope.cn/api/v1/models/RapidAI/RapidOCR/repo?Revision=v3.9.2&FilePath=onnx/PP-OCRv5/det/ch_PP-OCRv5_det_mobile.onnx)   |
| `ocr-cls.onnx`    | [Paddle angle classifier](https://huggingface.co/SWHL/RapidOCR/resolve/4e4644045a07c403b1ad40ca97340ecb4a8dc2c1/PP-OCRv1/ch_ppocr_mobile_v2.0_cls_infer.onnx)       |
| `ocr-v5-rec.onnx` | [PP-OCRv5 mobile recognition](https://www.modelscope.cn/api/v1/models/RapidAI/RapidOCR/repo?Revision=v3.9.2&FilePath=onnx/PP-OCRv5/rec/ch_PP-OCRv5_rec_mobile.onnx) |

检测与识别使用 PP-OCRv5 mobile ONNX 模型，来自 [RapidAI/RapidOCR 模型库](https://www.modelscope.cn/models/RapidAI/RapidOCR)，固定在 v3.9.2 并校验 SHA-256。文字方向分类继续使用原来的轻量分类器；识别字典从 ONNX 模型元数据加载。OCR 文件名与校验值在 `src-tauri/src/intelligence.rs`，库的模型和字典下载均已禁用。嵌入模型使用 [Qwen3-Embedding](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B) 的本地 ONNX 导出，与 OCR 共用 ONNX Runtime 1.28.0。分发模型时应保留对应模型许可证。

模型在首次执行相应的 OCR、索引或语义查询时加载，之后保留会话以供复用，任务结束或暂停不会自动卸载。更换向量模型、导入模型文件或退出应用时释放会话。Qwen 模型清单无效时，模型设置与语义搜索会报告错误，不影响打开资料库、图片导入、复制、普通设置或独立的 OCR；智能搜索仍可使用关键词。

### 选择大小与自行导出 Qwen

在「偏好设置 → Embedding → 向量模型」选择 0.6B、4B 或 8B，再导入对应的 `memechan-model.json`；清单引用的 ONNX、外部权重和 tokenizer 会一并复制，无需手动改名。导入前请将清单及其引用的文件放在同一目录。旧版单独导入 ONNX 和 tokenizer 的 0.6B 需要重新通过清单导入；已有 ONNX 可用下文的脚本生成清单，无需重新导出。0.6B 索引版本同时更新，旧索引会重新排队。

切换大小或导入同一大小的新权重，会释放旧推理会话并重新排队索引。原图、OCR、描述、分组和收藏不变；已经主动清除的索引仍需手动建立。不同大小、量化方式或 tokenizer 的向量不会混用。选择未安装的模型后，智能搜索暂用关键词；开启自动索引且任务未暂停时，模型导入完成便逐张重建，否则在任务管理中继续。更换模型前需暂停本地任务并等待当前图片完成。

所有大小均支持 FP32 或 INT8 模型包。本机导出的 0.6B FP32 权重约 2.38 GB（2.22 GiB），运行时还需要工作内存。当前应用仍使用 CPU 推理，并未接入 GPU。大模型加载、索引和搜索需要更多内存与时间；**导出和量化的峰值内存远高于运行 INT8 模型**，不要用“0.6B 推理很轻松”推断这台机器能直接导出 8B。

`scripts/export_qwen.py` 是独立的开发工具，使用 [Hugging Face Optimum 导出接口](https://huggingface.co/docs/optimum-onnx/onnx/package_reference/export) 和 [ONNX Runtime 动态量化](https://onnxruntime.ai/docs/performance/model-optimizations/quantization.html)。它只读取本地权重，关闭 Hub 网络访问；桌面应用不依赖 Python。先在有足够内存的机器准备 Qwen 官方 **Embedding** 权重目录（含 `config.json`、safetensors、tokenizer），以及独立的 Python 工具环境：

```bash
uv venv --python 3.14 .local/qwen-export-tools
uv pip install --python .local/qwen-export-tools/bin/python --torch-backend cpu \
  'torch==2.14.1' 'optimum-onnx[onnxruntime]==0.1.0' \
  'onnxruntime==1.31.0' 'onnx==1.23.2'
.local/qwen-export-tools/bin/python scripts/export_qwen.py \
  --source /path/to/Qwen3-Embedding-0.6B --size 0.6b \
  --output models/qwen3-embedding-0.6b-official-fp32 --precision fp32
```

4B / 8B 改为对应权重目录和 `--size 4b` / `--size 8b`。依赖安装可以使用联网环境或离线 wheel；脚本本身不下载模型，并关闭 Python Runtime 遥测。上面的转换工具版本已实测；Optimum ONNX 0.1.0 要求 Transformers `<4.58`，本次解析为 4.57.6。Python Runtime 1.31.0 与应用 Runtime 1.28.0 分别验证，导出产物采用 IR 8、opset 18 和无缓存的 feature-extraction；打包已有 ONNX 时由应用加载阶段检查 Runtime 对 IR 和算子的支持。

脚本默认导出 FP32。`--precision int8` 保留为实验选项，并保留 `fp32/` 中间产物：本次 0.6B 测试中，动态 INT8 与原始 PyTorch 向量出现明显偏移，尝试完整 8 位范围、保留词嵌入及有符号量化均未明显改善。简单检索测试通过并不足以证明量化质量，应使用自己的图片描述和查询验证。此流程尚未在本机完整导出 4B / 8B。

已从 Qwen 官方 0.6B 权重（提交 `97b0c614be4d77ee51c0cef4e5f07c00f9eb65b3`）完成导出。8 条样本覆盖短词、动作描述、中英文查询及 512 token 长文本，FP32 ONNX 与 PyTorch 的末 token 归一化向量余弦相似度最低为 `0.99999988`。推荐产物在本机 `models/qwen3-embedding-0.6b-official-fp32/`，选择其中的 `memechan-model.json` 导入；实验结果保存在 `models/qwen3-embedding-0.6b-export-experiments/validation.json`。这些目录均已被 Git 忽略。

如果已有自行导出的 ONNX，只需生成清单，不必重复导出。这个步骤只需 `onnx` Python 包：

```bash
python scripts/export_qwen.py --size 4b \
  --onnx /path/to/export/model.onnx \
  --tokenizer /path/to/Qwen3-Embedding-4B/tokenizer.json
```

脚本检查模型接口、输出维度与外部权重是否齐全，计算 SHA-256，并在 ONNX 所在目录生成清单。支持 `input_ids`、`attention_mask`、可选 `position_ids` / FP32 KV 输入，以及 FP32 的 `last_hidden_state` 或 `token_embeddings` 输出；应用执行末 token 池化与归一化。分词器必须对应原模型，保留特殊 token 处理。

模型包按内容指纹存放在 `models/qwen3-embedding-大小/指纹/`；所有文件校验通过后才切换当前清单。失败或中断的导入不会替换正在使用的模型，重新导入可复用已校验文件。旧版本目录暂时保留，确认新版本正常并退出应用后，可手动删除该型号目录中非当前清单对应的版本。清单校验用于检测文件损坏与区分索引版本，不证明模型来源；应使用自己的导出或可信来源。

已用真实 0.6B 和 4B INT8 模型验证清单生成、本地导入、向量维度、归一化和中英文检索排序。4B 测试使用 [Jasai 的 ONNX 转换](https://huggingface.co/Jasai/Qwen3-Embedding-4B-INT8-ONNX/tree/25095098411b94f4c5bdaf5f4596e7e41046de67)，不代表 4B 自导出流程已验证。8B 已接入模型选择与 4096 维处理，尚未运行真实权重测试。

从 v4 升级后，在「偏好设置 → OCR」导入约 21 MB 的 v5 检测与识别模型即可。原图、描述、收藏和分组保留，旧图片会重新排队识别并更新语义索引；重新识别前仍保留旧 OCR 文字用于关键词搜索。旧的 `ocr-det.onnx`、`ocr-rec.onnx` 不再使用，确认升级成功后可以手动删除。

## 数据与代码

### 图片理解 API

在「偏好设置 → 图片理解 API」填写地址、模型名和 API Key，保存后点击「测试图片理解」。测试发送内置的红色方块与蓝色圆形图片，不读取资料库；这是一次真实 API 调用，可能计费。

- 使用 OpenAI 兼容的 `POST /chat/completions`。地址可填 `https://api.deepseek.com`、`https://example.com/v1` 或完整接口地址；不会自动添加 `/v1`。支持本机 `http://localhost:端口/v1` 免密服务，远程服务需 HTTPS。
- 默认模型名为 `deepseek-v4-flash`。按照 [DeepSeek 当前文档](https://api-docs.deepseek.com/)，旧名称由当前 Flash 服务处理；详情会显示接口返回的模型名。其他服务需选择支持 `image_url` 的视觉模型。
- 地址、模型名、API Key 和自动生成开关保存在本机 SQLite 资料库中，API Key 未加密。重启后自动恢复配置，无需重新填写；设置中可显示、隐藏和修改密钥，留空并保存可清除。修改接口地址时会清空表单中的旧密钥。
- 在详情中点击「通过 API 生成描述」，或在「任务管理 → 图片理解 API」点击「生成缺失描述」。批量生成跳过已完成描述，待处理项可继续，失败项可重试。自动生成默认关闭；开启后仅为新导入的图片排队，重启后可直接调用；已有待处理队列可在任务管理中继续。
- 每次处理一张表情，压缩为最长边不超过 1024 px 的 JPEG。动图复用 OCR 抽帧规则，在前 180 帧中最多抽取 5 帧按时间顺序发送；原文件不变，不保证捕捉到较长动图的全部动作。
- 描述包含人物外观、动作、情绪、聊天语境和图片中清晰可辨的文字，文字提取结果保存在 API 描述中。可关闭本地 OCR，直接使用这些描述检索。保存后即支持关键词检索；开启自动索引且 Qwen 模型就绪时，自动更新向量。关闭自动索引时，可在任务管理中手动继续处理。
- 网络请求不持有资料库锁；暂停在当前请求完成后生效（请求超时 90 秒）。错误会显示并暂停队列，不会自动重复收费请求。已有描述和未完成队列保留，重新生成失败不会清空旧描述。

### 本地数据

数据放在 Tauri 的应用数据目录 `com.emojidesktop.app` 下，Linux 通常为 `~/.local/share/com.emojidesktop.app/`：

改名为 Memechan 后继续使用这个应用标识，已有图片、模型与设置无需迁移。

```text
library.sqlite3    元数据、分组、设置、文字与向量
originals/         导入后的原文件
thumbnails/        网格缩略图
copies/            复制副本缓存，自动回收旧文件
models/            本地模型
```

关闭应用后备份整个目录即可。删除资料库中的图片不删除最初导入的来源文件；删除分组也不删除图片。

复制缓存以 **512 MiB 为软上限**，启动时及运行期间每分钟检查一次，超限时按最近使用时间回收旧副本。重复复制会复用文件并刷新使用时间。清理保留当前剪贴板引用、本次运行中最近一次复制，以及最近 10 分钟使用过的文件；因此占用可能暂时超过上限。无法读取剪贴板时跳过清理并显示错误，原图不受影响。在「偏好设置 → 复制与资料库」可查看占用并手动清理，保护规则相同。系统剪贴板历史中的旧文件引用可能随缓存回收失效，重新从资料库复制即可。

前端与后端通过 `src/api.ts` 的 Tauri 命令交互，使用官方 `@tauri-apps/api`。主要代码职责如下：

- `src/App.vue`：资料库状态、搜索、编辑操作和页面组合；`src/components/`：网格、详情和偏好设置。设置表单维护独立草稿，后台刷新不会覆盖正在编辑的值。
- `src-tauri/src/app.rs`：应用状态、OCR / 索引及图片理解队列；不依赖 Tauri、GTK 或原生剪贴板。
- `inference_scheduler.rs`：本地推理调度，让交互搜索优先并取消过期的排队搜索，不中断已开始的推理。
- `desktop.rs`：Tauri 命令、文件选择及缓存定时清理，协调复制与清理的互斥；`clipboard.rs`：原生剪贴板及文件引用；`copy_cache.rs`：复制缓存占用、回收策略和清理状态。
- `import.rs`：读取、校验、解码图片并暂存原图与缩略图；这些操作在资料库锁外完成，入库时只短暂锁库进行去重、文件发布和数据库事务。
- `library.rs`：SQLite、元数据和检索，保存索引版本，不读取模型清单。应用层在启动、切换或导入模型时同步版本。向量在一次 SQL 查询中读取，不逐张查询数据库；仍然检查模型版本和文本来源，避免使用旧向量。
- `media.rs`：解码、动图抽帧、透明背景处理和复制副本；`intelligence.rs`：本地模型与推理；`vision.rs`：图片理解 API。

资料库、API 和队列测试放在对应模块的 `tests.rs` 中；`src-tauri/tests/models.rs` 保留需要真实模型的离线集成测试。`benchmarks/embeddings/` 是选型评测资料，不属于日常测试。复制尺寸不会改变检索内容。

TypeScript 保持 6.0.3，不升级到 7；当前 `@typescript-eslint` 支持范围为 `<6.1`。`Cargo.lock` 当前锁定 `ort 2.0.0-rc.13`，使用其默认分发的 ONNX Runtime 1.28.0；`rapid-ocr-rs` 子模块已适配同一版本。清单使用默认版本范围，后续通过更新锁文件升级；RC 之间的 API 变化仍需检查。Rust 的 `ort` 版本与原生 ONNX Runtime 版本分别管理，并非同一个版本号。

## 检查

```bash
pnpm format          # Prettier + rustfmt
pnpm format:check
pnpm lint            # ESLint + Vue / TypeScript 类型检查
pnpm build           # 前端生产构建
pnpm lint:rust       # Rust Clippy，需完整桌面依赖
pnpm test:core       # 去重、分组、索引失效、GIF 帧/时序/循环等核心行为
```

界面测试使用模拟桌面命令边界，不代表原生剪贴板或实际模型推理测试：

```bash
pnpm exec playwright-core install chromium
pnpm dev             # 保持运行，在另一个终端执行下面命令
pnpm test:ui
```

如果已有 Chromium，可以通过 `PLAYWRIGHT_CHROMIUM_EXECUTABLE` 指定路径，避免重复下载。
通过 `MEMECHAN_TEST_URL` 可以指定其他本地开发服务地址，默认使用 `http://127.0.0.1:1420/`。

真实 OCR 测试单独执行，先将上表的 OCR 文件按应用内文件名放入指定目录。测试只从本地导入到临时目录并推理，不会下载模型：

```bash
MEMECHAN_MODEL_TEST_DIR=/tmp/memechan-models \
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  --test models local_models_import_safely_and_support_ocr -- --ignored --nocapture
```

测试 Qwen 模型包时，将目录指向包含 `memechan-model.json` 的文件夹，并指定大小：

```bash
MEMECHAN_MODEL_TEST_DIR=/path/to/memechan-qwen4b MEMECHAN_EMBEDDING_MODEL=qwen3-4b \
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  --test models selected_qwen_model_supports_multilingual_search -- --ignored --nocapture
```

后台队列的真实模型集成测试也不需要桌面系统依赖。指定目录需同时包含上表的 OCR 文件及 0.6B 模型清单和关联文件：

```bash
MEMECHAN_MODEL_TEST_DIR=/tmp/memechan-models \
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib \
  app::tests:: -- --ignored --test-threads=1
```

已验证的范围：

- Prettier、rustfmt、ESLint、Vue / TypeScript 类型检查、前端生产构建。
- Rust Clippy（包括桌面代码），资料库与媒体核心行为测试。
- 图片理解队列与 HTTP 协议测试使用本机模拟服务，不连接付费 API；真实服务可通过设置中的「测试图片理解」手动检查。
- 本地模型分批导入、重复导入及错误文件拒绝；离线 PP-OCRv5 中文识别及 Qwen 语义排序测试。
- 索引逐条可见、模型切换后仅重建向量、待处理队列持久化、删除项和清除文字不会被后台结果恢复。桌面后台任务另有本地模型集成测试，覆盖批量追加、暂停、重启续做、失败项跳过，以及只安装 Qwen、关闭 OCR 时的独立索引与搜索。
- OCR 升级保留原图与元数据，重新索引一次，且不会使用文字变更前的旧向量。
- 界面自动化使用官方 Tauri mocks，覆盖编辑复制、设置模型、任务管理、批量导入、窗口切换和缓存清理；检查保存期间继续编辑的草稿、后台刷新、API 描述展示、单张与全库清除，以及界面未请求外部资源。虚拟网格使用 2,000 张模拟图片验证滚动、窗口缩放、紧凑模式、菜单和复制，任务仅更新进度时不重载资料库。
- Linux 开发版本构建、隔离 Xvfb 中原生窗口启动与文件导入；静图剪贴板尺寸及 GIF 剪贴板完整字节校验。

日常开发需按上方说明安装系统开发依赖。Discord、Telegram 与具体网页中的实际粘贴，以及 Windows/macOS 的运行尚未验证。
