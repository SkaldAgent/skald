# SHAPES & MEDIA — Shapes, Images, Styling

> Source: [iWork Automation — Shapes](https://iworkautomation.com/keynote/shape-line-shape.html), [Images](https://iworkautomation.com/keynote/image.html), [keynoteMP — shapes.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/shapes.ts), [images.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/images.ts), [theme.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/theme.ts)

---

## 4.1 Creating shapes with text inside

```applescript
tell application "Keynote"
    tell slide 1 of document 1
        -- Simple rectangle
        set myShape to make new shape with properties ¬
            {position:{80, 300}, ¬
             width:400, ¬
             height:200, ¬
             opacity:100, ¬
             object text:"Text inside the shape"}
        
        -- Styled text inside the shape
        tell myShape
            set font of its object text to "Helvetica Neue"
            set size of its object text to 24
            set color of its object text to {0, 0, 0}
            set alignment of its object text to center alignment
        end tell
    end tell
end tell
```

### Full-screen shape

```applescript
tell application "Keynote"
    tell front document
        set docW to its width    -- 1920
        set docH to its height   -- 1080
        tell current slide
            set fullShape to make new shape with properties ¬
                {position:{0, 0}, ¬
                 width:docW, ¬
                 height:docH, ¬
                 opacity:100}
        end tell
    end tell
end tell
```

### Centered shape with proportional dimensions

```applescript
tell application "Keynote"
    tell front document
        set docW to its width
        set docH to its height
        tell current slide
            set shapeW to docW * 0.75
            set shapeH to docH * 0.75
            set shapeX to (docW - shapeW) div 2
            set shapeY to (docH - shapeH) div 2
            set thisShape to make new shape with properties ¬
                {position:{shapeX, shapeY}, ¬
                 width:shapeW, ¬
                 height:shapeH, ¬
                 opacity:100, ¬
                 object text:"Centered text"}
        end tell
    end tell
end tell
```

### Shape properties

| Property | Type | Values |
| -------- | ---- | ------ |
| `position` | `{x, y}` | Top-left corner coordinates |
| `width` | integer | Width in points |
| `height` | integer | Height in points |
| `opacity` | integer | 0–100 |
| `rotation` | integer | 0–359 degrees |
| `reflection showing` | boolean | |
| `reflection value` | integer | 0–100 |
| `object text` | rich text | Contained text |

---

## 4.2 Fill: Color Fill

### ⚠️ Important AppleScript limitation

`background fill type` is **read-only** in AppleScript — the fill type of a shape cannot be changed via script.

**`set color of shapeRef to {R,G,B}` does NOT work in practice** (despite what some guides claim). The shape's fill color CANNOT be changed via AppleScript. Accept the default fill color.

If you need a colored placeholder, use a text item with a monospace label instead of a shape, or create the shape and leave it with the default fill.

### Border / Stroke

```applescript
tell application "Keynote"
    tell slide 1 of document 1
        set newShape to make new shape with properties ¬
            {position:{80, 300}, width:400, height:200}
        
        -- Border (stroke)
        set stroke color of newShape to {0, 0, 0}   -- Black
        set stroke width of newShape to 3            -- 3 points
    end tell
end tell
```

### Slide background (via JXA)

For the slide background, **JXA (JavaScript for Automation)** is more reliable:

```javascript
// JXA — JavaScript for Automation
var app = Application("Keynote");
var slide = app.documents[0].slides[0];
slide.backgroundColor = [50000, 20000, 10000]; // RGB Keynote 0-65535
```

### Slide background (AppleScript attempt)

```applescript
tell application "Keynote"
    tell slide 1 of front document
        set its background fill type to color fill
        set its background color to {50000, 20000, 10000}
    end tell
end tell
```

### Slide background with image (via JXA)

```javascript
// JXA
var app = Application("Keynote");
var slide = app.documents[0].slides[0];
slide.backgroundFillType = "image_fill";
slide.backgroundImage = Path("/Users/username/Desktop/background.jpg");
```

---

## 4.3 Hex → Keynote RGB Conversion

Colors in Keynote AppleScript use values **0–65535** per channel (16-bit). To convert from hex:

```applescript
on hexToKeynoteRGB(hexString)
    -- hexString like "#FF0000" or "#2563EB"
    set cleanHex to text 2 thru -1 of hexString
    set r to (do shell script "printf '%d' 0x" & text 1 thru 2 of cleanHex) as integer
    set g to (do shell script "printf '%d' 0x" & text 3 thru 4 of cleanHex) as integer
    set b to (do shell script "printf '%d' 0x" & text 5 thru 6 of cleanHex) as integer
    return {r * 257, g * 257, b * 257}
end hexToKeynoteRGB

-- Usage:
set color of newShape to my hexToKeynoteRGB("#2563EB")   -- Blue
set color of newShape to my hexToKeynoteRGB("#FF0000")   -- Red
set color of newShape to my hexToKeynoteRGB("#00FF00")   -- Green
```

### Named colors recognized by AppleScript

You can use these names instead of the RGB list:
`"black"`, `"blue"`, `"brown"`, `"cyan"`, `"green"`, `"magenta"`, `"orange"`, `"purple"`, `"red"`, `"yellow"`, `"white"`.

---

## 4.4 Adding images (file path)

Images require an HFS alias or POSIX file.

### From POSIX path (recommended for modern scripts)

```applescript
tell application "Keynote"
    tell slide 1 of document 1
        set imgFile to POSIX file "/Users/username/Desktop/photo.jpg"
        set newImage to make new image with properties ¬
            {file:imgFile, ¬
             position:{80, 300}, ¬
             width:600, ¬
             height:400}
    end tell
end tell
```

### From HFS alias (classic style)

```applescript
tell application "Keynote"
    tell slide 1 of document 1
        set imgFile to alias "Macintosh HD:Users:username:Desktop:photo.jpg"
        set newImage to make new image with properties ¬
            {file:imgFile, ¬
             position:{80, 750}, ¬
             width:400, ¬
             height:300}
    end tell
end tell
```

---

## 4.5 Resizing and positioning images

```applescript
tell application "Keynote"
    tell slide 1 of document 1
        set myImage to make new image with properties ¬
            {file:(POSIX file "/path/to/image.jpg")}
        
        tell myImage
            -- Resize
            set its width to 800
            set its height to 500
            
            -- Position
            set its position to {200, 200}
            
            -- Opacity
            set its opacity to 80
            
            -- Reflection
            set its reflection showing to true
            set its reflection value to 30   -- 0–100%
            
            -- Rotation
            set its rotation to 0
            
            -- Replace the displayed image
            set its file name to (POSIX file "/path/to/new-image.jpg")
        end tell
        
        -- Read image info
        tell myImage
            set imgPos to its position   -- → {x, y}
            set imgW to its width
            set imgH to its height
            set imgFile to its file name
        end tell
    end tell
end tell
```

### Image properties table

| Property | Type | Notes |
| -------- | ---- | ----- |
| `position` | `{x, y}` | Top-left corner |
| `width` | integer | Width in points |
| `height` | integer | Height in points |
| `opacity` | integer | 0–100 |
| `rotation` | integer | 0–359 degrees |
| `reflection showing` | boolean | |
| `reflection value` | integer | 0–100 |
| `file` | file (r/o) | Creation only |
| `file name` | text or file | Reads or replaces the image |
| `description` | text | VoiceOver text |

---

## 4.6 Example: full-slide image

```applescript
tell application "Keynote"
    activate
    set thisDocument to make new document with properties ¬
        {document theme:theme "Black", width:1920, height:1080}
    
    tell thisDocument
        set base slide of first slide to master slide "Blank"
        tell first slide
            -- Add image
            set imgFile to POSIX file "/Users/username/Desktop/background.jpg"
            set bgImage to make new image with properties {file:imgFile}
            
            tell bgImage
                -- Fit to slide height and center horizontally
                set height of it to 1080
                set thisItemWidth to its width
                set position of it to {(1920 - thisItemWidth) div 2, 0}
            end tell
            
            -- Text overlay
            set titleBox to make new text item with properties ¬
                {object text:"Title over image"}
            tell titleBox
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 72
                set color of its object text to {65535, 65535, 65535}
                set position of it to {80, 800}
                set width of it to 1000
            end tell
        end tell
    end tell
end tell
```

---

## Sources

- [iWork Automation — Shapes](https://iworkautomation.com/keynote/shape-line-shape.html)
- [iWork Automation — Images](https://iworkautomation.com/keynote/image.html)
- [keynoteMP — shapes.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/shapes.ts)
- [keynoteMP — images.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/images.ts)
- [keynoteMP — theme.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/theme.ts)
