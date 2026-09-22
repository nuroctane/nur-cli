# Chromatic studies

NurCLI has **41 themes**. This collection adds 18 palettes: twelve colored dark grounds and six tinted papers. Open `/theme`, type a name to filter, and use arrows to preview. Enter saves; Escape restores your previous theme. You can also set one directly, for example `/theme acid-orchid`.

These palettes use three deliberately contrasting pigments across headings, links, inline code, paths, highlights and the animated banner. Surfaces are derived from each ground and ink, while success, warning and error remain distinct. No theme changes the default Nur Gold selection.

| Theme ID | Ground | Main accent | Second pigment | Third pigment |
| --- | --- | --- | --- | --- |
| `acid-orchid` | `#190F25` | `#DBFF63` | `#F09CFA` | `#86E7EC` |
| `blood-orange` | `#260E18` | `#FFA06E` | `#83E5C6` | `#C7ADFF` |
| `petrol-peach` | `#082528` | `#FFBC94` | `#C0ACFF` | `#8FE1AB` |
| `ultraviolet-milk` | `#201339` | `#CBB6FF` | `#F3DF6F` | `#89E9CF` |
| `copper-candy` | `#281B18` | `#FF9BDD` | `#85DFCC` | `#E8CE84` |
| `radioactive-jam` | `#251127` | `#C9FA70` | `#FF9CB7` | `#ABBFFF` |
| `glacier-rust` | `#10232E` | `#83E8F2` | `#FFAC88` | `#EBE19D` |
| `black-sesame` | `#1B201C` | `#C8E69A` | `#EEB4CC` | `#B4DCF0` |
| `velvet-circuit` | `#271026` | `#99CAFF` | `#FFAAC2` | `#E7ED7B` |
| `infrared-tide` | `#092924` | `#FFA49E` | `#80E6D7` | `#CAB0FF` |
| `cobalt-saffron` | `#101C3C` | `#F3D372` | `#8FDAFA` | `#F4B0D0` |
| `bruise-bloom` | `#252032` | `#F6C397` | `#A1EFBE` | `#CBB6FF` |
| `pistachio-ink` | `#E6EFD2` | `#792963` | `#165C69` | `#8C3E19` |
| `rose-concrete` | `#F0DEDF` | `#155B49` | `#344896` | `#8B2546` |
| `butter-signal` | `#F6EDBF` | `#6333A0` | `#145D52` | `#96391E` |
| `lilac-ochre` | `#E9E1F1` | `#805018` | `#46428E` | `#285D45` |
| `apricot-carbon` | `#F4DFC9` | `#175B64` | `#67417C` | `#932F4B` |
| `blueprint-rose` | `#DCEAF2` | `#912D60` | `#314E94` | `#526019` |

The six light palettes start at `pistachio-ink`. Their green, blush, butter, lilac, apricot and blue papers use dark colored inks rather than bright accents designed for black backgrounds.

Every registered theme is rendered at 72 and 100 columns by the typography preview harness and checked against the 3:1 rendered contrast floor. The new collection also tests text roles against every surface and requires at least 4.5:1 for text on the selected accent. Personal accent overrides and transparent mode still apply; their contrast depends on your override and terminal background.

Developer preview: `NUR_TYPO_DUMP=.nur/typo cargo test --bin nur typography_preview -- --ignored`, followed by `python scripts/contrast_audit.py .nur/typo 3.0`.
