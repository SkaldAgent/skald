# BASIC — Keynote AppleScript Fundamental Syntax

> Main source: [iWork Automation Keynote](https://iworkautomation.com/keynote/) and [keynoteMP MCP server](https://github.com/superdwayne/keynoteMP)

---

## 1.1 Creating a document with a theme

The `document theme` property of the `document` object is **read/write** — it can be both read and changed.

```applescript
tell application "Keynote"
    activate
    
    -- Create document with specific theme (widescreen 1920×1080)
    set thisDocument to make new document with properties ¬
        {document theme:theme "Black", width:1920, height:1080}
    
    -- Standard 4:3 (1024×768) — default dimensions
    -- set thisDocument to make new document with properties ¬
    --     {document theme:theme "White"}
end tell
```

### Change theme on an existing document

```applescript
set document theme of front document to theme "Gradient"
```

### List available themes

```applescript
tell application "Keynote"
    set themeNames to the name of every theme
    -- → {"Black", "White", "Gradient", "Classic", "Modern Type", "Minimal", …}
end tell
```

### Find a specific theme by name (robust)

Themes are instances of the `theme` object. To find them safely:

```applescript
tell application "Keynote"
    set themeList to every theme
    set targetTheme to missing value
    repeat with t in themeList
        if name of t is "Black" then
            set targetTheme to t
            exit repeat
        end if
    end repeat
    if targetTheme is not missing value then
        make new document with properties {document theme:targetTheme}
    else
        error "Theme 'Black' not found"
    end if
end tell
```

### Document dimensions

| Format | Width | Height | Aspect Ratio |
| ------ | ----- | ------ | ----------- |
| Standard (4:3) | 1024 | 768 | 1.33 |
| Widescreen (16:9) | 1920 | 1080 | 1.78 |
| Cinema (16:10) | 1920 | 1200 | 1.60 |

```applescript
-- Read current dimensions
tell document 1
    set docW to its width
    set docH to its height
end tell
```

---

## 1.2 Master Slides — what they are and how to use them

Every theme contains a set of **master slides** (preset layouts). Every slide you create is based on one of these.

### List master slides of the current theme

```applescript
tell application "Keynote"
    tell document 1
        set masterSlideNames to the name of every master slide
        -- → {"Title & Subtitle", "Title - Center", "Title - Top", 
        --    "Title & Bullets", "Title, Bullets & Photo", "Bullets", 
        --    "Photo - Horizontal", "Photo - Vertical", "Photo - 3 Up",
        --    "Photo", "Quote", "Blank"}
    end tell
end tell
```

### Most common master slides

| Master Name | Default Title | Default Body | Use |
|------------|:---:|:---:|-----|
| `"Title & Subtitle"` | ✅ | ✅ | Opening slide |
| `"Title - Center"` | ✅ | ❌ | Centered title |
| `"Title - Top"` | ✅ | ❌ | Title at top |
| `"Title & Bullets"` | ✅ | ✅ | Content with bullets |
| `"Title, Bullets & Photo"` | ✅ | ✅ | Content + photo |
| `"Bullets"` | ❌ | ✅ | Bullets only |
| `"Blank"` | ❌ | ❌ | Blank canvas |
| `"Photo"` | ❌ | ❌ | Image only |
| `"Quote"` | ✅ | ✅ | Quote |

### Create a slide with a specific master

```applescript
tell application "Keynote"
    tell document 1
        set thisSlide to make new slide with properties ¬
            {base slide:master slide "Title & Bullets"}
    end tell
end tell
```

### ⚠️ WARNING: reference context

The reference `master slide "Title & Bullets"` must be made in the context of the `document`, NOT inside a `tell slide block`.

**WRONG** ❌
```applescript
tell slide 1
    set its base slide to master slide "Title - Center"
end tell
```

**CORRECT** ✅
```applescript
tell document 1
    set the base slide of the current slide to master slide "Title - Center"
end tell
```

**Alternative** — save the reference in a variable:
```applescript
tell document 1
    set thisMasterSlide to master slide "Title - Center"
    tell the current slide
        set the base slide to thisMasterSlide
    end tell
end tell
```

---

## 1.3 Default Title / Body Item

Each slide (except `"Blank"`) has two default text items:
- `default title item` — for the title
- `default body item` — for the body / subtitle / bullet list

```applescript
tell application "Keynote"
    tell document 1
        set base slide of first slide to master slide "Title & Subtitle"
        tell first slide
            set object text of default title item to "My Presentation"
            set object text of default body item to "An interesting subtitle"
        end tell
        
        -- Slide with bullet list (use return to separate)
        set newSlide to make new slide with properties ¬
            {base slide:master slide "Title & Bullets"}
        tell newSlide
            set object text of default title item to "Key Points"
            set object text of default body item to ¬
                "Point 1" & return & "Point 2" & return & "Point 3"
        end tell
    end tell
end tell
```

### Hide/Show the default text items

```applescript
tell application "Keynote"
    tell document 1
        set s to make new slide with properties ¬
            {base slide:master slide "Title & Bullets"}
        tell s
            set title showing to false   -- hides the default title
            set body showing to true     -- shows the body (already visible)
        end tell
    end tell
end tell
```

---

## 1.4 Creating Custom Text Items

Custom text items (added via script) behave differently from the defaults — they are "simplified": they grow horizontally to fit the text, then wrap.

```applescript
tell application "Keynote"
    tell document 1
        tell slide 1
            set thisTextItem to make new text item with properties ¬
                {object text:"Custom text", ¬
                 position:{80, 200}, ¬
                 width:600, ¬
                 height:100}
        end tell
    end tell
end tell
```

### Editable properties (inherited from iWork item)

| Property | Type | Notes |
| -------- | ---- | ----- |
| `position` | `{x, y}` | Coordinates from the top-left corner |
| `width` | integer | Width in points |
| `height` | integer | Height in points |
| `opacity` | integer | 0–100 |
| `rotation` | integer | 0–359 degrees |
| `reflection showing` | boolean | |
| `reflection value` | integer | 0–100 percent |
| `locked` | boolean | |

---

## 1.5 Font Size, Font Color, Font Name

### RGB Color System in Keynote

RGB colors in Keynote use values **0–65535** per channel (16-bit). Conversion from hex:

```
R = parseInt("#FF0000"[1..3], 16) * 257  → 65535
G = parseInt("#FF0000"[3..5], 16) * 257  → 0
B = parseInt("#FF0000"[5..7], 16) * 257  → 0
```

### Complete styling example

```applescript
tell application "Keynote"
    tell document 1
        set base slide of first slide to master slide "Blank"
        tell slide 1
            -- Create text item
            set thisTxt to make new text item with properties ¬
                {object text:"Styled Text", ¬
                 position:{80, 200}, width:600, height:80}
            
            tell thisTxt
                -- Font name (PostScript or display name)
                set font of its object text to "Helvetica Neue"
                -- "TimesNewRomanPS-ItalicMT", "Zapfino", etc.
                
                -- Font size in points
                set size of its object text to 48
                
                -- Color: {R, G, B} from 0 to 65535
                set color of its object text to {0, 0, 65535} -- Blue
                
                -- Bold and Italic
                set bold of its object text to true
                set italic of its object text to false
                
                -- Alignment
                set alignment of its object text to center alignment
                -- left alignment, center alignment, right alignment
            end tell
        end tell
    end tell
end tell
```

### Recognized named colors

These can be used instead of the RGB list:
`"black"`, `"blue"`, `"brown"`, `"cyan"`, `"green"`, `"magenta"`, `"orange"`, `"purple"`, `"red"`, `"yellow"`, `"white"`.

Example: `set color of its object text to "blue"`

---

## 1.6 Shapes — Creation, Positioning, Text

```applescript
tell application "Keynote"
    tell front document
        set docW to its width
        set docH to its height
        tell current slide
            -- Full-screen shape
            set fullShape to make new shape with properties ¬
                {position:{0, 0}, ¬
                 width:docW, ¬
                 height:docH, ¬
                 opacity:100}
            
            -- Shape with centered text inside
            set shapeW to docW * 0.75
            set shapeH to docH * 0.75
            set shapeX to (docW - shapeW) div 2
            set shapeY to (docH - shapeH) div 2
            set textShape to make new shape with properties ¬
                {position:{shapeX, shapeY}, ¬
                 width:shapeW, ¬
                 height:shapeH, ¬
                 opacity:100, ¬
                 object text:"Text inside shape"}
        end tell
    end tell
end tell
```

### ⚠️ Shape fill color — AppleScript limitation

`background fill type` is **read-only** in AppleScript. The fill type cannot be changed programmatically.

However, the keynoteMP code (which works in practice) uses `set color of shapeRef to {R,G,B}` for the fill color:

```applescript
set newShape to make new shape with properties ¬
    {position:{80, 300}, width:400, height:200}
set color of newShape to {65535, 0, 0}  -- Red
```

**Border/Stroke:**
```applescript
set stroke color of newShape to {0, 0, 0}     -- Black
set stroke width of newShape to 3              -- 3 points
```

---

## 1.7 Transitions

Transitions are an AppleScript record set on the `transition properties` property of the slide.

```applescript
tell application "Keynote"
    tell document 1
        tell slide 1
            set transition properties to ¬
                {transition effect:dissolve, ¬
                 transition duration:2.0, ¬
                 transition delay:1.5, ¬
                 automatic transition:true}
        end tell
    end tell
end tell
```

### Transition settings record properties

| Property | Type | Description |
| -------- | ---- | ----------- |
| `transition effect` | enum | One of 33 effects (see below) |
| `transition duration` | real | Seconds for the transition |
| `transition delay` | real | Seconds to wait before starting |
| `automatic transition` | boolean | `true` = automatic, `false` = on click |

### All 33 transition effects

`no transition effect`, `magic move`, `shimmer`, `sparkle`, `swing`, `dissolve`, `cube`, `flip`, `mosaic`, `push`, `reveal`, `switch`, `wipe`, `blinds`, `color planes`, `confetti`, `doorway`, `drop`, `fall`, `flop`, `iris`, `move in`, `object cube`, `object flip`, `object pop`, `object push`, `object revolve`, `object zoom`, `perspective`, `revolving door`, `scale`, `swoosh`, `twirl`, `twist`

---

## 1.8 Saving and Exporting the File

### Save

```applescript
tell application "Keynote"
    -- Save in-place
    save front document
    
    -- Save with specific name and location
    save front document in POSIX file "/Users/username/Desktop/presentation.key"
    
    -- Close with save
    close front document saving yes
    -- close front document saving no   -- without saving
    -- close front document saving ask  -- ask the user
end tell
```

### Export to other formats

```applescript
tell application "Keynote"
    -- Export PDF
    export document 1 to file ¬
        ((POSIX file "/Users/username/Desktop/presentation.pdf") as string) ¬
        as PDF
    
    -- Export PowerPoint
    export document 1 to file ¬
        ((POSIX file "/Users/username/Desktop/presentation.pptx") as string) ¬
        as Microsoft PowerPoint
    
    -- Export as images
    export document 1 to file ¬
        ((POSIX file "/Users/username/Desktop/Images/") as string) ¬
        as slide images with properties {image format:PNG}
    -- image format: PNG, JPEG, TIFF
end tell
```

---

## Sources

- [iWork Automation Keynote — Using Themes](https://iworkautomation.com/keynote/theme-doc-make.html)
- [iWork Automation Keynote — Master Slides](https://iworkautomation.com/keynote/slide-masters.html)
- [iWork Automation Keynote — Default Text Items](https://iworkautomation.com/keynote/slide-default-text.html)
- [iWork Automation Keynote — Text Item Styling](https://iworkautomation.com/keynote/text-item-styling.html)
- [iWork Automation Keynote — Text Items](https://iworkautomation.com/keynote/text-item.html)
- [iWork Automation Keynote — Slide Transitions](https://iworkautomation.com/keynote/slide-transition.html)
- [iWork Automation Keynote — Document](https://iworkautomation.com/keynote/document.html)
- [iWork Automation Keynote — Shapes](https://iworkautomation.com/keynote/shape-line-shape.html)
- [keynoteMP — presentation.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/presentation.ts)
- [keynoteMP — slides.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/slides.ts)
- [keynoteMP — text.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/text.ts)
- [keynoteMP — transitions.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/transitions.ts)
