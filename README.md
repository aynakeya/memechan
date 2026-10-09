# Memechan · 表情资料库

一个使用 **Tauri 2 + Vue 3 + Nuxt UI 4 + TypeScript + Vite + Rust + SQLite** 的本地桌面表情库。没有 Electron，也不需要 Python 服务。

界面使用 Nuxt UI 的 Vue/Vite 模式，统一按钮、表单、菜单、弹窗和通知，不需要 Nuxt 服务端。主题在 `vite.config.ts` 和 `src/style.css` 中配置；使用系统字体，Lucide 图标在构建时从本地依赖打包，运行时不请求字体或图标服务。

## 当前实现

- 拖入或选择 PNG、JPEG、GIF、WebP / APNG，复制原文件到资料库，按 SHA-256 去重。
- 分组、收藏、最近添加、未分组、动图筛选、原图预览，以及自定义描述。
- 双击复制，右键选择原尺寸、128 / 256 / 512 px 或自定义最长边；默认尺寸可设置，保持比例，可选择是否放大小图。
- 静图写入图片和文件剪贴板格式。动图写入原始文件与对应格式，不以静态首帧代替动画。
- GIF 缩放逐帧处理；动态 WebP / APNG 缩放后转换成 GIF。原文件始终保留。
- 本地 PP-OCRv5 mobile 中文、英文和日文识别；动图在前 180 帧中最多抽样 5 帧并合并识别文字。
- Qwen3-Embedding-0.6B INT8 多语言文本向量检索：文件名、自定义描述、OCR 文字作为检索内容。使用查询指令、末个有效 token 的 1024 维向量、L2 归一化和余弦相似度；智能搜索通过倒数排名融合关键词与向量结果。
- 模型可从本地文件导入，也可手动联网下载；固定版本、模型及 tokenizer 的 SHA-256 校验；本地保存索引，可暂停任务、重试失败项目。

自动图片描述、直接理解无文字图片的视觉语义、云同步暂未实现。当前没有文字且没有自定义描述的图片只能依赖文件名检索。

## 启动

使用 pnpm 11.12.0 管理依赖，版本固定在 `package.json` 的 `packageManager`。需要 Node.js 22.13+（推荐 24）、Rust 1.90+、C/C++ 构建工具和各平台的 [Tauri 开发依赖](https://v2.tauri.app/start/prerequisites/)。

Ubuntu / Debian 先安装开发包：

```bash
sudo apt install build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev patchelf
```

然后：

```bash
pnpm install
pnpm tauri dev
```

`pnpm dev` 只打开浏览器界面预览。浏览器中明确禁用本地文件、剪贴板和模型功能，真实功能需要 Tauri 桌面窗口。第一次编译需下载 Rust 依赖和 ONNX Runtime。

在对应平台打包：

```bash
pnpm tauri build
```

跨平台代码和图标已准备，当前验证了 Linux 开发版本。Windows/macOS 的构建、签名和粘贴兼容性仍需在对应环境验证。ONNX Runtime 由 `ort` 按目标平台链接；发布安装包时还需核对动态运行库是否完整分发。

## 使用

把图片拖入窗口即可添加；打开分组后导入，会加入该分组。单击查看描述、OCR 和所属分组，双击复制，空格预览，Ctrl / Cmd K 搜索。描述可以显式保存，也会在切换所选图片时保存。

在「偏好设置 → 本地搜索模型」点击「导入本地模型」，可多选下表中的 ONNX 和 tokenizer 文件，自动识别内容并复制到资料库，无需重命名，不移动原文件。支持分批导入，界面会列出缺少的文件；文件齐全后会验证加载并建立索引。之后导入图片、修改描述会自动在后台更新索引；关闭自动索引后，可以手动处理。已完成索引的图片立即参与语义搜索，无需等待整批完成；模型未安装时智能搜索使用关键词匹配，纯语义搜索会明确提示。

大量导入时，每张图片独立保存并加入待处理队列，导入期间会更新资料库和进度。后台一次只处理一张图片，逐张保存 OCR 和向量，失败项不会阻塞后续图片。暂停后新增图片仍可导入，但不会自动解除暂停；点击「继续处理待索引表情」恢复。重启时，开启自动索引且模型就绪会继续 SQLite 中的未完成项。尚未保存的导入文件需要重新选择；已导入文件会去重。

设置的尺寸表示图片最长边；填 `0` 使用原尺寸。动图转换为 GIF 会受 GIF 色彩、透明度和时间精度限制。首次缩放可能需要等待，再次复制使用缓存。

**动图粘贴取决于接收方。** Discord、Telegram 桌面版、各浏览器及具体网页对文件剪贴板的支持不同；不能保证任意网页编辑框都接受 GIF。Linux 上还取决于 X11 / Wayland 与桌面剪贴板协议。当前实现写入动图文件，不静默降级为静图，需要在实际目标软件里检查发送后的动画。

## 离线模型

本地文件导入、模型加载、识别和搜索不需要外部请求，ONNX Runtime 的遥测在初始化时显式关闭。只有展开「可选：联网下载」并点击「联网下载模型」才会连接 ModelScope 与 Hugging Face；不会自动下载，图片和描述不会上传。推理在本地 CPU 执行，每个会话限制为 2 个线程，图片逐张处理。

模型目录在偏好设置中显示。可以从其他机器拷贝以下文件，再通过应用导入。若直接手动复制到模型目录，则需按左栏重命名并重启应用。合计约 647 MB。

当前导入仅支持下表固定版本的模型文件，会拒绝损坏或不匹配的文件。任意 ONNX 模型不能通过改文件名直接替换：不同模型的分词器、输入输出、向量维度和处理方式需要对应适配。

| 本地文件名                  | 固定版本下载地址                                                                                                                                                             |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ocr-v5-det.onnx`           | [PP-OCRv5 mobile detection](https://www.modelscope.cn/api/v1/models/RapidAI/RapidOCR/repo?Revision=v3.9.2&FilePath=onnx/PP-OCRv5/det/ch_PP-OCRv5_det_mobile.onnx)            |
| `ocr-cls.onnx`              | [Paddle angle classifier](https://huggingface.co/SWHL/RapidOCR/resolve/4e4644045a07c403b1ad40ca97340ecb4a8dc2c1/PP-OCRv1/ch_ppocr_mobile_v2.0_cls_infer.onnx)                |
| `ocr-v5-rec.onnx`           | [PP-OCRv5 mobile recognition](https://www.modelscope.cn/api/v1/models/RapidAI/RapidOCR/repo?Revision=v3.9.2&FilePath=onnx/PP-OCRv5/rec/ch_PP-OCRv5_rec_mobile.onnx)          |
| `qwen3-embedding-0.6b.onnx` | [Qwen3-Embedding-0.6B quantized](https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX/resolve/c25a394dd583836952667c12f008335071b3f43d/onnx/model_quantized.onnx) |
| `qwen3-tokenizer.json`      | [Qwen tokenizer](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B/resolve/97b0c614be4d77ee51c0cef4e5f07c00f9eb65b3/tokenizer.json)                                           |

检测与识别使用 PP-OCRv5 mobile ONNX 模型，来自 [RapidAI/RapidOCR 模型库](https://www.modelscope.cn/models/RapidAI/RapidOCR)，固定在 v3.9.2 并校验 SHA-256。文字方向分类继续使用原来的轻量分类器，与 `paddle-ocr-rs` 的 v5 示例一致；识别字典从 ONNX 模型元数据加载。模型清单与校验值在 `src-tauri/src/intelligence.rs`。嵌入模型来自 [Qwen3-Embedding](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B) 的 [ONNX Community 转换](https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX)，沿用当前 ONNX Runtime 1.22。分发模型时应保留对应模型许可证。

从 BGE 升级只需导入 Qwen 权重和分词器，现有 PP-OCRv5 文件继续使用。旧向量会被移除并逐张重建，原图、描述、OCR 文字、收藏和分组保留。模型文件使用独立名称，不会把旧 BGE 分词器误判为 Qwen 已安装。当前发布配置支持 0.6B；后续 4B / 8B 需要对应的已验证 ONNX 文件清单和向量维度配置，更换时必须重建索引，不能混用不同模型的向量。

从 v4 升级后，在「偏好设置 → 本地搜索模型」导入或联网下载约 21 MB 的 v5 检测与识别模型即可。原图、描述、收藏和分组保留，旧图片会重新排队识别并更新语义索引；重新识别前仍保留旧 OCR 文字用于关键词搜索。旧的 `ocr-det.onnx`、`ocr-rec.onnx` 不再使用，确认升级成功后可以手动删除。

## 数据与代码

数据放在 Tauri 的应用数据目录 `com.emojidesktop.app` 下，Linux 通常为 `~/.local/share/com.emojidesktop.app/`：

改名为 Memechan 后继续使用这个应用标识，已有图片、模型与设置无需迁移。

```text
library.sqlite3    元数据、分组、设置、文字与向量
originals/         导入后的原文件
thumbnails/        网格缩略图
copies/            复制时生成的文件，持续保留以免未粘贴的剪贴板失效
models/            本地模型
```

关闭应用后备份整个目录即可。删除资料库中的图片不删除最初导入的来源文件；删除分组也不删除图片。复制缓存暂不自动回收，可以在关闭应用、完成粘贴后清理 `copies/`。

前端与后端通过 `src/api.ts` 的 Tauri 命令交互，使用官方 `@tauri-apps/api` 的命令、文件 URL 和拖拽事件接口。`library.rs` 管理 SQLite 与导入；`media.rs` 处理原图、动画和复制副本；`intelligence.rs` 负责模型与推理；`desktop.rs` 连接桌面能力及后台任务。后续自动描述可作为额外的索引文本来源接入，复制尺寸不会改变检索内容。

依赖使用兼容的稳定版本。TypeScript 保持 6.0.3，不升级到 7；当前 `@typescript-eslint` 支持范围为 `<6.1`。`ort` 固定为 `2.0.0-rc.10`：`paddle-ocr-rs 0.6.1` 仍访问 `Session.inputs` 公共字段，而 `ort rc.13` 已将其改为私有字段与访问方法，需等待 OCR 库适配后再升级。

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

真实模型测试单独执行，先将上表所需文件按应用内文件名放入指定目录。测试只从本地导入到临时目录并推理，不会下载模型：

```bash
MEMECHAN_MODEL_TEST_DIR=/tmp/memechan-models \
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  --test models -- --ignored --nocapture
```

后台队列的真实模型集成测试需要桌面系统依赖，但不打开窗口：

```bash
MEMECHAN_MODEL_TEST_DIR=/tmp/memechan-models \
  cargo test --manifest-path src-tauri/Cargo.toml --lib \
  desktop::tests::bulk_import_joins_active_worker_and_resumes_after_restart -- --ignored
```

已验证的范围：

- Prettier、rustfmt、ESLint、Vue / TypeScript 类型检查、前端生产构建。
- Rust Clippy（包括桌面代码），资料库与媒体核心行为测试。
- 本地模型分批导入、重复导入及错误文件拒绝；离线 PP-OCRv5 中文识别及 Qwen 语义排序测试。
- 索引逐条可见、模型切换后仅重建向量、待处理队列持久化、删除项不会被后台结果恢复。桌面后台任务另有本地模型集成测试，覆盖批量追加、暂停、重启续做和失败项跳过。
- OCR 升级保留原图与元数据，重新索引一次，且不会使用文字变更前的旧向量。
- 浏览器中图片显示、拖拽导入、描述保存、复制尺寸、默认设置、动图筛选、预览关闭和搜索错误恢复，使用官方 Tauri mocks；检查界面未请求外部资源。
- Linux 开发版本构建、隔离 Xvfb 中原生窗口启动与文件导入；静图剪贴板尺寸及 GIF 剪贴板完整字节校验。

日常开发需按上方说明安装系统开发依赖。Discord、Telegram 与具体网页中的实际粘贴，以及 Windows/macOS 的运行尚未验证。
