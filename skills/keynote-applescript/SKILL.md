# Keynote AppleScript Skill

_Updated: 2026-06-05_

## What it does

Generates Keynote presentations via AppleScript. Creates documents, slides, text items, shapes, images, transitions and saves `.key` files.

## When to use

- The user asks for a Keynote / PowerPoint presentation
- They want a quick draft to edit afterwards
- The Mac has Keynote installed (even if renamed, e.g. "Keynote Creator Studio")

## How to invoke

Write an AppleScript file (`.applescript`) and run it:

```bash
osascript path/to/script.applescript
```

## Quick Reference

### Create document with theme

```applescript
tell application "Keynote" -- or "Keynote Creator Studio"
    make new document with properties {document theme:theme "Slate"}
end tell
```

### Create a slide with a master

```applescript
tell document 1
    set thisSlide to make new slide with properties {base slide:master slide "Title & Bullets"}
end tell
```

### Custom text item

```applescript
tell slide 1
    set myText to make new text item with properties {object text:"Hello"}
    set position of myText to {100, 200}
    set width of myText to 400
    set height of myText to 60
    set size of object text of myText to 24
    set color of object text of myText to {65535, 65535, 65535}
end tell
```

### Shape

```applescript
tell slide 1
    set s to make new shape
    set position of s to {100, 100}
    set width of s to 400
    set height of s to 300
    set object text of s to "Placeholder label"
    set size of object text of s to 16
end tell
```

### Transition

```applescript
tell slide 1
    set transition properties to {transition effect:dissolve, transition duration:1.0}
end tell
```

### Save

```applescript
save document 1 in file (POSIX file "/Users/username/Desktop/Filename.key")
```

## Documentation Files

The following files contain detailed, structured documentation. Read the one relevant to your task before writing the script.

| File | Size | Contents |
|------|------|----------|
| [01-basic.md](01-basic.md) | 427 lines | Document creation, master slides, default/custom text items, font/style, basic shapes, transitions, save/export |
| [02-layout.md](02-layout.md) | 297 lines | **Critical for layout**: coordinate system, safe margins (top=60, left=80 for 1920×1080), content area 1760×960, Y-accumulator, 8-point grid spacing |
| [03-slide-master.md](03-slide-master.md) | 250 lines | Master slide table per theme, hiding defaults with `title showing`/`body showing`, combining default + custom items, switching masters |
| [04-shapes-media.md](04-shapes-media.md) | 324 lines | Shapes with text, ⚠️ fill color NOT settable via AS, images (POSIX file), resizing |
| [05-recipes.md](05-recipes.md) | 399 lines | **5 complete working examples**: title slide, bullet+photo, text+screenshot, two-column custom layout, serial slide generator |

## ⚠️ Important Notes

### Known AppleScript pitfalls (LEARNED THE HARD WAY)

1. **`object text` compound property FAILS inside handlers** — do NOT write helper functions that use `set object text of itemRef to txt`. It compiles but crashes at runtime. **Inline everything instead.**

2. **`set color of shape to {R,G,B}` does NOT work** — shape fill color cannot be set via AppleScript despite what some guides claim. Accept the default fill. For placeholders, use a text item with a dashed-label or just an empty shape.

3. **Avoid reserved keywords as variable names**: `text`, `width`, `height`, `color`, `by`, `and`, `or`, `div`, `mod`. AppleScript is case-insensitive so `bY` = `by` = reserved word. Use prefixes like `_t`, `_b`, `_p`, `_sY`.

4. **Better to create text items then set properties** than using `make new text item with properties {object text:"..."}`. The `object text` label in a properties record can cause parsing issues:
   ```applescript
   -- DO THIS:
   set myItem to make new text item
   set object text of myItem to "Hello"
   set position of myItem to {80, 60}
   
   -- NOT THIS (may fail):
   set myItem to make new text item with properties {object text:"Hello", position:{80, 60}}
   ```

5. **Test compile with `osacompile` before running** — AppleScript errors are cryptic. Always `osacompile -o /tmp/test.scpt script.applescript` first.

6. **Always check available master slides** for the specific theme:
   ```applescript
   tell document 1 to set masterNames to name of every master slide
   ```
   Names vary between themes (e.g. "Title & Subtitle" may not exist; use "Title" instead).

### General notes

- The app name may be "Keynote" or "Keynote Creator Studio" — verify before running
- Theme/master names may vary between themes and locales
- Use `POSIX file` for modern file paths
- **`size`** and **`color`** go on **`object text`** of the item, not directly on the text item
- ALWAYS read [02-layout.md](02-layout.md) before writing a multi-slide script to avoid overlapping elements
