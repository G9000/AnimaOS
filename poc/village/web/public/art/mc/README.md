# Player-supplied main character

The user selected this pink-haired character as the fixed protagonist on 2026-10-25, replacing the procedural character creator. These are original files copied unchanged from `C:/Users/leoca/Downloads/Julio MC/`:

| Project file | Supplied source |
| --- | --- |
| `portrait.png` | `Julio MC.png` (384×576) |
| `spritesheet.png` | `MC_spritesheet.png` (96×384), the matching transparent atlas alongside the supplied GIFs |
| `idle.gif` | `MC_sprite_Idle.gif` (two 300 ms frames; opaque preview background) |
| `walking.gif` | `MC_sprite_walking.gif` (four 300 ms frames) |

The atlas is rendered directly, preserving its original pixels and transparency. No image generation, redraw, background removal or format conversion was applied. The portrait and GIFs are retained as source/reference assets; runtime animation uses the original transparent atlas.

The atlas has 32×48 cells, three columns and eight rows. Idle rows 0–3 are front/back/right/left with columns 0/1. Walking rows 4–7 use the same direction order and columns 0/1/2. Runtime sequences are idle `[0,1]` and walk `[0,1,2,1]`, each frame lasting 300 ms. Every cell uses the same in-texture crop `[3,6,26,34]`, keeping all hair/backpack/foot pixels and a consistent baseline. The texture uses nearest filtering and sRGB.

The folder label is retained for provenance; it does not establish the character's in-game name. Existing save names and story progress are preserved as legacy data.
