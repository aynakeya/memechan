# Memechan Logo

白发紫瞳的 Q 版龙娘抱着企鹅形态的表情包。由内置 `image_gen` 工具参考用户提供的四张图片生成，透明背景 PNG 原图保存在 `memechan-logo.png`。

桌面图标由 Tauri 官方 CLI 从原图转换为 PNG、ICO 和 ICNS，位于 `src-tauri/icons/`；界面与 favicon 使用 `public/memechan-icon.png`。

## 生成提示词

```text
Use case: logo-brand
Asset type: transparent square mascot logo for the desktop meme library Memechan.
Primary request: Design a polished, compact anime chibi girl hugging a penguin meme with both arms, using the four attached images as visual references. This is a new logo composition.
Input images: Image 1 is the character design reference (white-haired purple-eyed dragon girl); Image 2 is the chibi proportion/style reference for that white-haired girl only; Images 3 and 4 are references for the absurd Tomori Takamatsu penguin meme being hugged. Do not reproduce their backgrounds or scene.
Subject: A very cute super-deformed white-haired girl with pale curved dragon horns, large lavender eyes, tiny blush and a quietly pleased little smile. Simplify her white and pale-lilac high-collar outfit to modest fully covered chibi clothing. One simple flower-knot hair clip recalls the reference. Tiny lavender dragon wings may frame her shoulders. Her oversized head is above and slightly beside the penguin. Both little hands visibly wrap around and gently squeeze the penguin from the sides.
The hugged penguin must be clearly a meme character, not an emoji circle: plump black/charcoal penguin body, cream belly, tiny flippers and stubby feet, a dark gray short anime bob with uneven bangs, a small human anime face with reddish pink eyes and a tiny dazed open mouth, echoing Images 3 and 4. Girl and penguin faces both fully visible and readable. The funny penguin should occupy about the lower 40 percent of the logo.
Style: clean Japanese chibi mascot sticker illustration, bold smooth dark plum outer outlines, simpler thinner internal strokes, restrained flat cel shading. Silver white, pale lavender and dark charcoal derived from references, small pink blush accents. Simple large shapes, a strong unified rounded silhouette readable at app-icon sizes, not a full-length detailed character illustration.
Composition: single centered tightly composed mascot pair, near-square arrangement; entire horns and feet visible, safe transparent margin of about 7 percent on all sides. No text, no wordmark, no panels, no sheet of variations, no scenery, no hearts or sparkles, no ground shadow, no app tile background. Genuine transparent alpha background.
```
