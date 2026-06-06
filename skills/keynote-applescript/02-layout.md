# LAYOUT — Coordinates, Spacing, Positioning

> Source: [keynoteMP Design Tokens](https://github.com/superdwayne/keynoteMP/blob/main/src/design/tokens.ts), [Grid System](https://github.com/superdwayne/keynoteMP/blob/main/src/design/grid.ts), [Balance Utility](https://github.com/superdwayne/keynoteMP/blob/main/src/design/balance.ts) and [iWork Automation](https://iworkautomation.com/keynote/)

---

## 2.1 How coordinates work

- **Origin (0,0)**: **top-left** corner of the slide
- **X**: distance from the left edge (increases to the right)
- **Y**: distance from the top edge (increases downward)
- **Units**: Keynote points (1 pt = 1 px at 72 dpi)
- **Position**: AppleScript record `{X, Y}` — e.g. `{80, 60}`

```
(0,0) ──────────────────────────────────── X →
  │
  │   ┌──────────────────────────────┐
  │   │  Content area                │
  Y │   │                              │
  │   │                              │
  ↓   └──────────────────────────────┘
```

---

## 2.2 Standard slide dimensions

| Format | Width | Height | Aspect Ratio | Use |
| ------ | ----- | ------ | ----------- | --- |
| Standard (4:3) | 1024 | 768 | 1.33 | Default, older projectors |
| Widescreen (16:9) | 1920 | 1080 | 1.78 | **Recommended**, modern screens |
| Cinema (16:10) | 1920 | 1200 | 1.60 | MacBook, some monitors |

Set during creation:

```applescript
make new document with properties ¬
    {document theme:theme "Black", width:1920, height:1080}
```

Read current dimensions:

```applescript
tell document 1
    set docW to its width    -- 1920
    set docH to its height   -- 1080
end tell
```

---

## 2.3 Real margins — how much space is actually available

> **Design rule**: NEVER place content beyond the safe margins.

Recommended "safe" margins (from the keynoteMP design system):

| Margin | 1920×1080 (16:9) | 1024×768 (4:3) |
| ------ | ---------------- | -------------- |
| Top | 60 | 40 |
| Bottom | 60 | 40 |
| Left | 80 | 60 |
| Right | 80 | 60 |
| **Content area** | **1760 × 960** | **904 × 688** |

The **content area** = everything that fits within the margins. This is where content should be placed.

### Calculate the content area

```applescript
-- Constants for widescreen 1920x1080
set marginTop to 60
set marginBottom to 60
set marginLeft to 80
set marginRight to 80

tell document 1
    set contentX to marginLeft                            -- 80
    set contentY to marginTop                             -- 60
    set contentW to (its width) - marginLeft - marginRight  -- 1760
    set contentH to (its height) - marginTop - marginBottom -- 960
end tell
```

---

## 2.4 How to calculate Y to avoid overlaps

### Y accumulator system

**Golden rule**: each element occupies `Y + height + gap`. Keep track of `currentY`.

```applescript
-- Setup
set marginTop to 60
set marginLeft to 80
set contentW to 1760
set contentH to 960

-- Y accumulator
set currentY to marginTop   -- start: 60
set gap to 24               -- md spacing

-- Element 1: Title (height ~60pt)
set titleHeight to 60
set titleY to currentY
-- ... create element at {marginLeft, titleY} with height titleHeight
set currentY to currentY + titleHeight + gap  -- now 144

-- Element 2: Subtitle (height ~40pt)
set subtitleHeight to 40
set subtitleY to currentY
-- ... create element at {marginLeft, subtitleY}
set currentY to currentY + subtitleHeight + gap  -- now 208

-- Element 3: Body text (fills all remaining space)
set bodyY to currentY
set bodyHeight to contentH - currentY + marginTop
-- ... create element at {marginLeft, bodyY} with height bodyHeight
```

### Visual diagram of the Y accumulator

```
Y=0  ┌────────────────────────────────┐
     │░░░░░░ TOP MARGIN (60pt) ░░░░░░░│
Y=60 │┌─ TITLE (h=60pt) ────────────┐│
Y=120││                               ││
     │└──────────────────────────────┘│
     │░░ gap md = 24pt ░░░░░░░░░░░░░░│
Y=144│┌─ SUBTITLE (h=40pt) ─────────┐│
Y=184││                               ││
     │└──────────────────────────────┘│
     │░░ gap md = 24pt ░░░░░░░░░░░░░░│
Y=208│┌─ BODY TEXT ──────────────────┐│
     ││                                │
     ││  (fills to the bottom)        ││
     ││                                ││
Y=960│└────────────────────────────────┘│
     │░░░░░░ BOTTOM MARGIN (60pt) ░░░░│
Y=1080└────────────────────────────────┘
```

---

## 2.5 Recommended spacing between elements (vertical)

From the keynoteMP **8-point grid** system:

| Token | Points | Recommended use |
| ----- | ------ | --------------- |
| xs | 8 | Between very close elements (icons, labels) |
| sm | 16 | Between body text and caption, between bullet items |
| md | 24 | **Between title and subtitle**, between paragraphs |
| lg | 32 | Between different sections, between subtitle and body |
| xl | 48 | Between slide title and main content |
| xxl | 64 | Between major content blocks |
| xxxl | 96 | Large visual separations (section break) |

### Practical recommendations

| Relationship | Gap | Token |
| ------------ | :-: | :---: |
| Between title and subtitle | **24pt** | md |
| Between subtitle and body | **32pt** | lg |
| Between body and image | **48pt** | xl |
| Between bullet list items | **16pt** | sm |
| Slide top margin | **60pt** | — |
| Side margin | **80pt** | — |

---

## 2.6 Layout examples with calculated coordinates

### Standard layout: content slide (1920×1080)

```applescript
-- Layout constants
set leftMargin to 80
set topMargin to 60
set contentWidth to 1760   -- 1920 - 80 - 80
set contentHeight to 960   -- 1080 - 60 - 60

-- Title: top-left, 60% area width
make new text item with properties ¬
    {object text:"Title", ¬
     position:{leftMargin, topMargin}, ¬
     width:contentWidth * 0.6, ¬
     height:60}

-- Subtitle: below title with md gap (24pt)
make new text item with properties ¬
    {object text:"Subtitle", ¬
     position:{leftMargin, topMargin + 60 + 24}, ¬
     width:contentWidth * 0.6, ¬
     height:40}

-- Body: below subtitle with lg gap (32pt)
set bodyY to topMargin + 60 + 24 + 40 + 32
set bodyHeight to contentHeight - (bodyY - topMargin)
make new text item with properties ¬
    {object text:"Body text...", ¬
     position:{leftMargin, bodyY}, ¬
     width:contentWidth * 0.55, ¬
     height:bodyHeight}
```

### Two-column layout (1920×1080)

```applescript
-- Two columns with 40pt gutter
set leftMargin to 80
set topMargin to 60
set gutter to 40
set colWidth to ((1920 - 80 - 80) - gutter) / 2  -- 840pt per column
set col1X to leftMargin           -- 80
set col2X to col1X + colWidth + gutter   -- 80 + 840 + 40 = 960

-- Common header (full width)
set headerY to topMargin
make new text item with properties ¬
    {object text:"Header", ¬
     position:{leftMargin, headerY}, ¬
     width:1760, height:60}

-- Left column
set bodyY to headerY + 60 + 32  -- + title + lg gap
make new text item with properties ¬
    {object text:"Left column content...", ¬
     position:{col1X, bodyY}, ¬
     width:colWidth, height:500}

-- Right column
make new text item with properties ¬
    {object text:"Right column content...", ¬
     position:{col2X, bodyY}, ¬
     width:colWidth, height:500}
```

### Text + image on the right layout (1920×1080)

```applescript
-- Text column: 55% width, image column: 40% + gutter
set leftMargin to 80
set topMargin to 60
set gutter to 40
set textColW to 1760 * 0.55     -- 968
set imageColW to 1760 * 0.40    -- 704
set imageColX to leftMargin + textColW + gutter  -- 80 + 968 + 40 = 1088

-- Title (text column)
make new text item with properties ¬
    {object text:"Section Title", ¬
     position:{leftMargin, topMargin}, ¬
     width:textColW, height:60}

-- Body (below the title)
set bodyY to topMargin + 60 + 24
make new text item with properties ¬
    {object text:"Descriptive text...", ¬
     position:{leftMargin, bodyY}, ¬
     width:textColW, height:500}

-- Image (right column)
set imgFile to POSIX file "/Users/username/Desktop/photo.jpg"
make new image with properties ¬
    {file:imgFile, ¬
     position:{imageColX, topMargin}, ¬
     width:imageColW, height:600}
```

---

## 2.7 Helper function for Hex → Keynote RGB conversion

```applescript
on hexToKeynoteRGB(hexString)
    -- hexString like "#FF0000" for red
    set cleanHex to text 2 thru -1 of hexString
    set r to (do shell script "printf '%d' 0x" & text 1 thru 2 of cleanHex) as integer
    set g to (do shell script "printf '%d' 0x" & text 3 thru 4 of cleanHex) as integer
    set b to (do shell script "printf '%d' 0x" & text 5 thru 6 of cleanHex) as integer
    return {r * 257, g * 257, b * 257}
end hexToKeynoteRGB

-- Usage: set color of object text of default title item to my hexToKeynoteRGB("#2563EB")
```

---

## Sources

- [keynoteMP — tokens.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/design/tokens.ts) — design tokens, margins
- [keynoteMP — grid.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/design/grid.ts) — 12-column grid system
- [keynoteMP — balance.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/design/balance.ts) — balance utility
- [iWork Automation — Document](https://iworkautomation.com/keynote/document.html) — document dimensions
