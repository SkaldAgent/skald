# RECIPES — Fully Working Examples

> Source: [iWork Automation — Examples](https://iworkautomation.com/keynote/examples.html), [keynoteMP](https://github.com/superdwayne/keynoteMP), and synthesis of all previous sections.

---

## 5.1 Opening slide: Title + Subtitle + Footer

### Professional layout with "Title & Subtitle" master

```applescript
-- PROFESSIONAL OPENING SLIDE
-- Master: "Title & Subtitle" with custom text item for footer

tell application "Keynote"
    activate
    
    -- Create widescreen document
    set thisDoc to make new document with properties ¬
        {document theme:theme "Black", width:1920, height:1080}
    
    tell thisDoc
        -- Set master for first slide
        set base slide of first slide to master slide "Title & Subtitle"
        
        tell first slide
            -- Main title (default)
            set object text of default title item to "Our Vision"
            
            -- Subtitle (default body)
            set object text of default body item to "Innovation for the digital future"
            
            -- Style the title
            tell default title item
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 60
                set color of its object text to {65535, 65535, 65535}
            end tell
            
            -- Style the subtitle
            tell default body item
                set font of its object text to "Helvetica Neue Light"
                set size of its object text to 36
                set color of its object text to {40000, 40000, 50000}
            end tell
            
            -- Custom footer (bottom right)
            set footerItem to make new text item with properties ¬
                {object text:"© 2026 Company Name", ¬
                 position:{1500, 1000}, ¬
                 width:340, ¬
                 height:30}
            tell footerItem
                set font of its object text to "Helvetica Neue"
                set size of its object text to 16
                set color of its object text to {30000, 30000, 30000}
            end tell
        end tell
        
        -- Transition for the first slide
        tell first slide
            set transition properties to ¬
                {transition effect:dissolve, ¬
                 transition duration:1.5, ¬
                 automatic transition:true, ¬
                 transition delay:0}
        end tell
    end tell
    
    -- Save
    tell thisDoc
        save thisDoc in POSIX file "/Users/username/Desktop/presentation.key"
    end tell
end tell
```

### Result

- Large title "Our Vision" in white (Helvetica Neue Bold 60pt)
- Subtitle "Innovation for the digital future" light grey (Helvetica Neue Light 36pt)
- Footer "© 2026 Company Name" bottom right (16pt)
- Dissolve transition on opening

---

## 5.2 Title + Bullet List + Sidebar Image

### Content layout with image on the right

```applescript
-- CONTENT SLIDE WITH IMAGE ON THE RIGHT
tell application "Keynote"
    activate
    tell document 1
        -- Create new slide with appropriate master
        set contentSlide to make new slide with properties ¬
            {base slide:master slide "Title, Bullets & Photo"}
        
        tell contentSlide
            -- Title
            set object text of default title item to "Our Services"
            tell default title item
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 42
            end tell
            
            -- Bullet list (use return to separate)
            set object text of default body item to ¬
                "Strategic consulting" & return & ¬
                "Software development" & return & ¬
                "Cloud infrastructure" & return & ¬
                "Team training"
            tell default body item
                set font of its object text to "Helvetica Neue"
                set size of its object text to 24
                set color of its object text to {10000, 10000, 10000}
            end tell
            
            -- Add image in the master's photo area
            set imgFile to POSIX file "/Users/username/Desktop/services.jpg"
            set serviceImage to make new image with properties ¬
                {file:imgFile, ¬
                 position:{1100, 200}, ¬
                 width:700, ¬
                 height:500}
        end tell
        
        -- Transition
        tell contentSlide
            set transition properties to ¬
                {transition effect:push, ¬
                 transition duration:1.0}
        end tell
    end tell
end tell
```

---

## 5.3 Title + Body Text + Screenshot on the Right

### Fully custom layout (master "Blank")

```applescript
-- FULLY CUSTOM SLIDE
-- Layout: text on the left (55%), screenshot on the right (40%)
-- Master: "Blank" for full control

tell application "Keynote"
    activate
    tell document 1
        set ds to make new slide with properties ¬
            {base slide:master slide "Blank"}
        
        tell ds
            -- Layout constants (1920x1080)
            set leftColX to 80
            set rightColX to 1080  -- 80 + 920 + 40 (gutter) = ~1080
            set topY to 80
            set colWidth to 920
            set rightColW to 760
            
            -- TITLE (left column, top)
            set titleItem to make new text item with properties ¬
                {object text:"New Feature", ¬
                 position:{leftColX, topY}, ¬
                 width:colWidth, ¬
                 height:70}
            tell titleItem
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 48
                set color of its object text to {0, 0, 0}
            end tell
            
            -- BODY TEXT (left column, below title)
            set bodyItem to make new text item with properties ¬
                {object text:"The new feature allows automating report creation " & ¬
                 "processes, reducing time by 60%." & return & return & ¬
                 "Key features:" & return & ¬
                 "• Interactive dashboard" & return & ¬
                 "• Automatic PDF export" & return & ¬
                 "• API integration" & return & ¬
                 "• Real-time notifications", ¬
                 position:{leftColX, topY + 70 + 24}, ¬
                 width:colWidth, ¬
                 height:500}
            tell bodyItem
                set font of its object text to "Helvetica Neue"
                set size of its object text to 22
                set color of its object text to {12000, 12000, 12000}
            end tell
            
            -- SCREENSHOT (right column)
            set imgFile to POSIX file "/Users/username/Desktop/screenshot.png"
            set screenshot to make new image with properties ¬
                {file:imgFile, ¬
                 position:{rightColX, topY}, ¬
                 width:rightColW, ¬
                 height:600}
        end tell
    end tell
end tell
```

### Layout diagram

```text
|←──── 920pt ────→|← 40 →|←── 760pt ──→|
┌───────────────────┬─────┬──────────────┐
│ TITLE (48pt)      │     │              │
│                   │     │  SCREENSHOT  │
│ Body text         │ gap │  (760×600)   │
│ with bullet list  │     │              │
│                   │     │              │
│ • Dashboard       │     │              │
│ • Export PDF      │     │              │
│ • API             │     │              │
└───────────────────┴─────┴──────────────┘
```

---

## 5.4 Empty Slide with Custom Elements

### Two-column layout with dividers and colored header

```applescript
-- FULLY CUSTOM SLIDE: TITLE + TWO COLUMNS + DIVIDERS
tell application "Keynote"
    activate
    tell document 1
        set ds to make new slide with properties ¬
            {base slide:master slide "Blank"}
        
        tell ds
            -- HEADER: Colored bar at top (full-width)
            set headerBar to make new shape with properties ¬
                {position:{0, 0}, width:1920, height:8, opacity:100}
            set color of headerBar to {60000, 30000, 0}  -- Orange
            
            -- TITLE (large, centered)
            set ttl to make new text item with properties ¬
                {object text:"Our Team", ¬
                 position:{80, 60}, ¬
                 width:1760, ¬
                 height:80}
            tell ttl
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 54
                set color of its object text to {0, 0, 0}
                set alignment of its object text to center alignment
            end tell
            
            -- DIVIDER: horizontal line below title
            set dividerLine to make new shape with properties ¬
                {position:{640, 160}, width:640, height:2, opacity:100}
            set color of dividerLine to {50000, 50000, 50000}
            
            -- LEFT COLUMN (40%)
            set leftTitle to make new text item with properties ¬
                {object text:"Design", ¬
                 position:{80, 220}, ¬
                 width:700, ¬
                 height:50}
            tell leftTitle
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 32
                set color of its object text to {60000, 30000, 0}  -- Orange
            end tell
            
            set leftText to make new text item with properties ¬
                {object text:"Our design team creates intuitive and appealing user " & ¬
                 "experiences, focusing on usability and aesthetics.", ¬
                 position:{80, 280}, ¬
                 width:700, ¬
                 height:200}
            tell leftText
                set font of its object text to "Helvetica Neue"
                set size of its object text to 22
                set color of its object text to {15000, 15000, 15000}
            end tell
            
            -- RIGHT COLUMN (40%)
            set rightTitle to make new text item with properties ¬
                {object text:"Development", ¬
                 position:{900, 220}, ¬
                 width:700, ¬
                 height:50}
            tell rightTitle
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 32
                set color of its object text to {0, 30000, 60000}  -- Blue
            end tell
            
            set rightText to make new text item with properties ¬
                {object text:"The development team builds robust and scalable " & ¬
                 "solutions using the most modern and performant technologies.", ¬
                 position:{900, 280}, ¬
                 width:700, ¬
                 height:200}
            tell rightText
                set font of its object text to "Helvetica Neue"
                set size of its object text to 22
                set color of its object text to {15000, 15000, 15000}
            end tell
            
            -- VERTICAL DIVIDER between columns
            set vDivider to make new shape with properties ¬
                {position:{855, 220}, width:1, height:200, opacity:100}
            set color of vDivider to {40000, 40000, 40000}
            
            -- FOOTER: small centered text at bottom
            set footer to make new text item with properties ¬
                {object text:"Company Inc. | info@company.com | Tel: +1 800 123 4567", ¬
                 position:{80, 1000}, ¬
                 width:1760, ¬
                 height:30}
            tell footer
                set font of its object text to "Helvetica Neue"
                set size of its object text to 14
                set color of its object text to {35000, 35000, 35000}
                set alignment of its object text to center alignment
            end tell
        end tell
    end tell
end tell
```

### Layout diagram — two columns

```text
┌──────────────────────────────────────────┐  Y=0
│▌▌▌▌▌▌▌▌▌▌▌▌▌▌▌ ORANGE BAR ▌▌▌▌▌▌▌▌▌▌▌▌▌│  Y=8
│                                          │
│               OUR TEAM                   │  Y=60–140
│                                          │
│     ──────────── DIVIDER ──────────      │  Y=160–162
│                                          │
│  DESIGN ──────────── │ DEVELOPMENT       │  Y=220
│  Descriptive text    │ Descriptive text  │  Y=280–480
│  left column         │ right column      │
│                                          │
│                                          │
│  ──────────────── FOOTER ──────────────  │  Y=1000
└──────────────────────────────────────────┘  Y=1080
```

---

## 5.5 Quick serial slide generator

To create multiple similar slides from a list:

```applescript
-- SLIDE GENERATOR FROM A LIST
tell application "Keynote"
    activate
    tell document 1
        set slideData to {¬
            {title:"Introduction", body:"Project overview"}, ¬
            {title:"Objectives", body:"• Reduce costs by 30%\n• Increase efficiency\n• Improve quality"}, ¬
            {title:"Timeline", body:"Q1 2026: Analysis\nQ2 2026: Development\nQ3 2026: Testing\nQ4 2026: Launch"}, ¬
            {title:"Conclusions", body:"Thank you for your attention"} ¬
        }
        
        repeat with slideInfo in slideData
            set slideTitle to item 1 of slideInfo
            set slideBody to item 2 of slideInfo
            
            set newSlide to make new slide with properties ¬
                {base slide:master slide "Title & Bullets"}
            tell newSlide
                set object text of default title item to slideTitle
                set object text of default body item to slideBody
                
                -- Consistent style
                tell default title item
                    set font of its object text to "Helvetica Neue Bold"
                    set size of its object text to 40
                end tell
                tell default body item
                    set font of its object text to "Helvetica Neue"
                    set size of its object text to 22
                end tell
            end tell
        end repeat
    end tell
end tell
```

---

## Sources

- [iWork Automation — Examples](https://iworkautomation.com/keynote/examples.html)
- [keynoteMP — slides.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/slides.ts)
- [keynoteMP — text.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/text.ts)
- [keynoteMP — shapes.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/shapes.ts)
- [keynoteMP — images.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/images.ts)
