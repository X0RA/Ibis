# ibis application icon

Black-and-white ibis head inspired by the user-supplied Australian white ibis
photo. Created with the built-in image generation tool and exported with
ImageMagick. The reference photo is not included in the repository.

- `appicon-1024.png`: transparent master for macOS iconsets and future exports.
- `appicon.png`: 256px PNG for the app window and Linux desktop installation.
- `appicon.ico`: Windows 16/32/48/64/128/256px icon.
- macOS CI generates `appicon.icns` from the master using sips/iconutil.

Generation prompt:

> Use case: logo-brand. Asset: desktop application icon for an app named ibis. The attached photo is an anatomical reference only: an Australian white ibis with a black bare head, small eye, very long slender downward-curved black bill and white neck. Create a new striking black-and-white logo of this ibis head and upper neck in side profile facing left. Bold clean flat vector-like silhouette, elegant curved bill fully visible, subtle white eye detail, white neck contrasting with black head. Square composition, generous safe margin, readable at 32px. Black and white only, no color, no text or letters, no mountains or photo-frame imagery, no photoreal textures, no mockup. Use a simple white rounded-square backing with a fine black edge so the black bird remains visible on a dark desktop; transparent outside the rounded square.

Final refinement prompt (built-in image generation):

> Edit this ibis app icon with one small targeted change: shorten the visible white neck by about 20%, making the head sit slightly lower within the rounded-square tile. Keep the ibis head shape, long curved black bill, eye, feather linework style, side-profile direction, black-and-white palette, rounded white backing and black edge essentially identical. Keep the bill fully visible. Preserve the overall icon size, margins and transparent exterior. Do not add text or any other elements. The existing icon is almost perfect; make only this subtle neck-proportion adjustment.
