# Original village artwork

Generated with the built-in image_gen tool on 2026-10-25. Original characters and artwork. All generated PNG assets preserve their transparent alpha; no image editing or format conversion was applied.

## Asset layout

- `tavi-portraits.png`: 2172 × 724 pixels; three 724 × 724 expression columns from left to right: warm neutral, thoughtful, happy. Same adult character, clothing and identity throughout.
- `characters.png`: 1774 × 887 pixels; two rows and four columns. Row 0: Tavi. Row 1: traveler/player. Columns: front, left, back, right. Pixel-art styled idle poses. Generated spacing is slightly uneven; use the alpha-tight rectangles below for centered sprites.

Rectangles are [x, y, width, height] measured from the top left, using alpha > 128. Add a few pixels of padding if needed for soft edge pixels.

| Character | Direction | Rectangle |
| --- | --- | --- |
| Tavi | front | [205, 23, 181, 416] |
| Tavi | left | [576, 18, 172, 423] |
| Tavi | back | [1012, 18, 172, 421] |
| Tavi | right | [1401, 18, 168, 423] |
| Traveler | front | [172, 456, 250, 417] |
| Traveler | left | [575, 456, 195, 419] |
| Traveler | back | [973, 456, 250, 416] |
| Traveler | right | [1376, 457, 193, 418] |

Use nearest-neighbor texture filtering for the game sprites. To maintain equal apparent height when cropping each sprite, scale the planes to the source rectangle aspect ratio.

## Portrait generation prompt

```text
Use case: illustration-story
Asset type: transparent dialogue portrait expression strip for an original cozy 2.5D browser RPG.
Primary request: Draw the same original adult village shopkeeper Tavi in exactly three equal-width columns, waist-up, expression variants from left to right: neutral warm welcome, thoughtful listening, happy smile. All three are the same person, outfit, face and scale.
Subject: approachable young adult shopkeeper, clearly adult, warm brown short slightly tousled hair tied in a tiny low ponytail, soft amber eyes, cream collared shirt with sleeves rolled to forearms, dark teal practical apron with simple brown leather straps, modest working clothes. Natural friendly proportions and expressive eyes. No held props.
Style/medium: polished hand-drawn 2D anime game character illustration, elegant crisp ink contours, soft painterly shading, subtle warm amber highlights, gentle woodland village palette, commercially finished character art.
Composition/framing: wide 3:1 landscape canvas, exactly THREE equally sized invisible columns. One waist-up portrait centered inside each column. Identical top-of-head and waist baseline across all three. Each portrait fully contained in its column with ample clear padding, shoulders and arms fully visible, no overlap between characters. Front-facing slight three-quarter body angle. Identical camera and crop. Hands comfortably near waist.
Lighting/mood: warm welcoming lantern glow, cozy and calm.
Scene/backdrop: genuinely transparent background, clean alpha cutout around each portrait, no background painting, no checkerboard painted into artwork.
Constraints: Exactly three portraits of Tavi. No text, no labels, no grid lines, no borders, no scenery, no watermark, no unrelated objects. Keep hair, face, amber eyes and dark teal apron identical in every column.
```

## Directional sprite generation prompt

The portrait strip above was passed as the identity reference image.

```text
Use case: stylized-concept
Asset type: production transparent pixel-art game character directional sprite atlas for a cozy 2.5D village RPG.
Input images: Image 1 is Tavi's identity reference only. Translate that same adult shopkeeper's brown hair, small low ponytail, cream shirt, dark teal apron and warm personality into crisp pixel art.
Primary request: Exactly EIGHT isolated full-body character sprites in a precise regular grid of TWO ROWS and FOUR COLUMNS on a genuinely transparent background. Every grid cell is equal. Top row: Tavi. Bottom row: adult traveler/player. Columns from left to right: front-facing towards viewer, left-facing profile, back-facing away from viewer, right-facing profile. Same exact character design across all four directions of each row.
Tavi: adult village shopkeeper from reference, brown short tousled hair and tiny low ponytail, cream rolled sleeve shirt, dark teal apron with brown straps tied at waist, brown trousers and dark brown practical shoes.
Traveler: friendly clearly adult village visitor, short dark brown hair, ochre hooded traveling cape, muted sage shirt, brown trousers and boots, small dark leather cross-body satchel. Different silhouette and palette from Tavi.
Style/medium: authentic crisp pixel art with visible square pixels, approximately 48×72 logical pixel character detailing enlarged cleanly; attractive polished RPG sprite art with simple warm limited palette and tidy dark outlines, no painterly texture, no anti-aliased soft blur. Chibi game sprite proportions, 3 heads tall, adult clothing and design, readable at small game scale. Quiet standing idle poses with arms close to body.
Composition/framing: landscape canvas 2:1 aspect ratio. Exactly 4 evenly spaced columns and 2 evenly spaced rows. Invisible grid; no drawn separators. Each character centered in its individual cell with 20 percent transparent padding on all sides, entire head and feet visible. Identical scale and foot baseline in every cell of a row. No sprite overlaps, no ground shadows, no scenery. Direction ordering matters: FRONT, LEFT PROFILE, BACK, RIGHT PROFILE. Repeat this order in both rows.
Constraints: transparent alpha background; no text, numbers, labels, grid lines, borders, weapons, sparkles, extra objects, watermarks, logos. Exactly eight characters total, clean usable sprite atlas.
```

## Mira expansion assets

Generated on 2026-10-25 using the built-in image_gen tool, with real transparent alpha preserved and no image editing or format conversion. Mira is an original adult character. These are project-bound source assets:

- `mira-portraits.png`: 2172 × 724 pixels, three 724 × 724 columns: warm, thoughtful, happy. Tavi's portrait strip was used only as a style reference.
- `mira-sprites.png`: 2172 × 724 pixels, front/left/back/right in four cells. The new Mira portrait strip was its identity reference. Alpha-tight rectangles (x, y, width, height): `[225,19,275,687]`, `[725,19,247,688]`, `[1203,19,258,688]`, `[1709,19,233,688]`.
- The earlier procedural traveler and creator were retired when the user selected the fixed MC. The first-playable traveler row remains in `characters.png` as a historical source asset. The current protagonist uses the [user-supplied MC assets](mc/README.md), with original four-direction idle and walking frames.

### Mira portrait prompt

```text
Use case: illustration-story. Asset type: original cozy browser RPG visual-novel character portrait expression sheet. The attached image is a STYLE REFERENCE ONLY for painterly anime rendering, warm muted palette, line work and equal portrait composition. Create a DIFFERENT adult character named Mira: a friendly practical village supper organizer in her early thirties, warm medium-brown skin, dark charcoal hair in one long loose braid over one shoulder with a small rust ribbon, hazel eyes, cream linen blouse under a terracotta cardigan and moss green skirt/sash. Clearly adult, modest clothing, no jewelry requirement. Three equal-width square cells arranged horizontally in a 3:1 canvas, each containing the SAME Mira at exactly the same scale, head to upper thighs with full head and hair inside cell, hands around waist, bottom crop consistent. Left cell warm welcoming smile, center cell thoughtful gently concerned face with one hand resting at chin, right cell delighted grateful smile. Fully transparent background in every cell. Hair and skin must be consistent across all three. Leave clear transparent margins at both sides of each portrait so adjacent characters do not overlap. No text, no labels, no border, no props obscuring the face, no environment. Match overall polish and brush texture of reference, while Mira's dark braid and terracotta clothing clearly distinguish her from Tavi. Preserve real alpha.
```

### Mira sprite prompt

```text
Use case: stylized-concept. Asset type: transparent production directional pixel-art sprite atlas for cozy 2.5D RPG. Image1 is the IDENTITY reference for Mira, not an edit target. Create exactly FOUR isolated full-body pixel-art sprites of the SAME adult Mira in a single horizontal row, four equal invisible square cells, wide 4:1 canvas if possible otherwise wide 3:1. Direction order left to right FRONT, LEFT PROFILE, BACK, RIGHT PROFILE. Adult Mira has warm medium-brown skin, charcoal dark hair in one thick long braid with a rust ribbon, cream linen blouse, terracotta cardigan, moss-green skirt and brown boots. Same palette/clothing/person in each cell. Crisp visibly square pixel art with dark outline and warm limited palette, about48x72 logical pixels enlarged cleanly, 3-heads-tall cozy RPG proportions and adult clothing. Standing neutral idle poses arms at side, no object in hands. Each full head and every boot contained inside its equal cell, matching foot baseline and apparent height, ample transparent padding so no sprite overlaps another. Background truly transparent alpha, no ground shadow, no painted checkerboard. No text, labels, lines, borders, scenery, glow, weapons, extra people or watermark. Not painterly. Each sprite centered exactly inside its quarter of the sheet.
```
