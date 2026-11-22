# The photographed instrument

The UI uses the owner's two Synthphone photographs. Preserve its real keys,
plastic, dust, uneven labels, handwriting, wear and proportions. The photos are
the instrument's visible surface; CSS must not draw replacement keys or a shell.

The built-in image editing tool produced three transparent layers:

- `phone-on-hook.png`: remove the photographed room, keep the complete phone.
- `phone-base.png`: remove only the receiver and uncover the cradle, keeping
  the base at the same coordinates.
- `receiver.png`: isolate only the receiver, in its original canvas position.

The prompts explicitly required photographic identity, original writing and
wear, genuine alpha, unchanged perspective, and no redesign or studio rendering.
The base and receiver use matching 1306×1205 canvases; the source cutout is
1305×1205 and is normalized by CSS. WebP files are lossless `cwebp -lossless
-exact -m 6` encodings of these PNGs. `wood.webp` is a lossless copy of the
existing K2 viewer's `assets/keyboard/wood-grain.png` texture.

`src/photo_geometry.rs` places small, accessible hit areas over the photographic
keycaps and encoder. Pressed keys show those same photo pixels, darkened and
slightly depressed. The OLED canvas occupies the photographed display slot.
