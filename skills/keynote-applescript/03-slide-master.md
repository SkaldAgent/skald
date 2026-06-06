# SLIDE MASTER — Understanding and Controlling Preset Layouts

> Source: [iWork Automation — Master Slides](https://iworkautomation.com/keynote/slide-masters.html), [Default Text Items](https://iworkautomation.com/keynote/slide-default-text.html), [keynoteMP — theme.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/theme.ts)

---

## 3.1 What are master slides

Every Keynote theme contains a set of **master slides** (also called "layouts" or "slide designs"). Every slide you create is based on one of these masters.

The master determines:

1. **Which default text items** are visible (title, body)
2. The **initial position and formatting** of these items
3. **Decorative background elements** (colors, gradients, shapes)
4. The **structure** of the slide (where photos, bullets, quotes go)

### Hierarchy

```text
Theme (e.g. "Black")
  └── Master Slide "Title & Subtitle"
        └── Slide 1 (based on that master)
  └── Master Slide "Blank"  
        └── Slide 2 (based on Blank)
  └── Master Slide "Title & Bullets"
        └── Slide 3
  └── ...
```

---

## 3.2 Common master slides list

### Most common masters table

| Master Name | Default Title | Default Body | Ideal use |
| ----------- | :-----------: | :----------: | --------- |
| `"Title & Subtitle"` | ✅ Visible | ✅ Visible | **Opening slide** — title + subtitle |
| `"Title - Center"` | ✅ Visible | ❌ Hidden | **Centered title**, without body |
| `"Title - Top"` | ✅ Visible | ❌ Hidden | **Title at top**, for custom slides |
| `"Title & Bullets"` | ✅ Visible | ✅ Visible | **Content** with title + bullet list |
| `"Title, Bullets & Photo"` | ✅ Visible | ✅ Visible | **Content + photo** with image placeholder |
| `"Bullets"` | ❌ Hidden | ✅ Visible | **Bullets only**, without title |
| `"Photo - Horizontal"` | ❌ Hidden | ❌ Hidden | **Horizontal photo** full screen |
| `"Photo - Vertical"` | ❌ Hidden | ❌ Hidden | **Vertical photo** |
| `"Photo - 3 Up"` | ❌ Hidden | ❌ Hidden | **Three photos** in a grid |
| `"Photo"` | ❌ Hidden | ❌ Hidden | **Single photo** |
| `"Quote"` | ✅ Visible | ✅ Visible | **Quote** — title = quote, body = author |
| `"Blank"` | ❌ Hidden | ❌ Hidden | **Blank canvas** — no default elements |

### How to discover the current theme's masters

```applescript
tell application "Keynote"
    tell document 1
        set masterNames to name of every master slide
        -- → {"Title & Subtitle", "Title - Center", "Title - Top", ...}
    end tell
end tell
```

---

## 3.3 How to hide/remove default title/body

The boolean properties `title showing` and `body showing` of the slide control the visibility of the default text items.

### Hide both (custom elements only)

```applescript
tell application "Keynote"
    tell document 1
        -- Create slide based on "Title & Bullets"
        set s to make new slide with properties ¬
            {base slide:master slide "Title & Bullets"}
        tell s
            set title showing to false   -- hides default title
            set body showing to false    -- hides default body
            -- Now you can add only custom text items
        end tell
    end tell
end tell
```

### Use only default body, hide title

```applescript
tell application "Keynote"
    tell document 1
        set s to make new slide with properties ¬
            {base slide:master slide "Title & Subtitle"}
        tell s
            set title showing to false
            set body showing to true
            set object text of default body item to "Body only visible"
        end tell
    end tell
end tell
```

### Alternative: use the "Blank" master

The `"Blank"` master has no visible default text items — it is the cleanest choice for fully customized slides:

```applescript
tell application "Keynote"
    tell document 1
        set s to make new slide with properties ¬
            {base slide:master slide "Blank"}
        -- On Blank you can only create custom text items and shapes
        tell s
            set myTitle to make new text item with properties ¬
                {object text:"Custom Title", ¬
                 position:{80, 60}, width:1000, height:60}
        end tell
    end tell
end tell
```

---

## 3.4 Using default title/body combined with custom text items

The hybrid approach is often the most powerful: use default text items (which have professional positioning from the theme) and enrich with custom text items.

### Example: default title + custom subtitle + hidden default body

```applescript
tell application "Keynote"
    tell document 1
        set s to make new slide with properties ¬
            {base slide:master slide "Title & Bullets"}
        tell s
            -- Default title: we use it for the main title
            set object text of default title item to "Main Title"
            tell default title item
                set font of its object text to "Helvetica Neue Bold"
                set size of its object text to 48
            end tell
            
            -- We hide the default body (not needed)
            set body showing to false
            
            -- We add a custom text item as subtitle
            set subItem to make new text item with properties ¬
                {object text:"Custom subtitle", ¬
                 position:{80, 200}, ¬
                 width:800, ¬
                 height:50}
            tell subItem
                set font of its object text to "Helvetica Neue Light"
                set size of its object text to 28
                set color of its object text to {30000, 30000, 30000}
            end tell
            
            -- We add a second custom text block
            set bodyItem to make new text item with properties ¬
                {object text:"Additional content...", ¬
                 position:{80, 270}, ¬
                 width:800, ¬
                 height:400}
        end tell
    end tell
end tell
```

### Example: default title + default body (bullet) + custom image

```applescript
tell application "Keynote"
    tell document 1
        set s to make new slide with properties ¬
            {base slide:master slide "Title & Bullets"}
        tell s
            -- Default title
            set object text of default title item to "Our Services"
            
            -- Default body with bullet list
            set object text of default body item to ¬
                "Strategic consulting" & return & ¬
                "Software development" & return & ¬
                "Cloud & infrastructure"
            
            -- Custom text item for a footnote
            set footnote to make new text item with properties ¬
                {object text:"* ISO 9001 certified services", ¬
                 position:{80, 900}, ¬
                 width:600, ¬
                 height:30}
            tell footnote
                set font of its object text to "Helvetica Neue"
                set size of its object text to 12
                set color of its object text to {40000, 40000, 40000}
            end tell
        end tell
    end tell
end tell
```

---

## 3.5 Changing the master slide of an existing slide

You can change the master of a slide **after** creating it:

```applescript
tell application "Keynote"
    tell document 1
        -- Correct method: reference in the document context
        set the base slide of the current slide to master slide "Title - Center"
        
        -- Alternative method with variable
        set thisMasterSlide to master slide "Blank"
        set the base slide of slide 3 to thisMasterSlide
    end tell
end tell
```

### How to get the current master name of a slide

```applescript
tell application "Keynote"
    tell document 1
        set masterName to name of base slide of slide 1
        -- → "Title & Subtitle" (or other)
    end tell
end tell
```

---

## 3.6 Best Practices with Master Slides

1. **Choose the closest master** to the desired layout, then customize
2. **"Blank" is for fully custom layouts** — no default elements to hide
3. **"Title & Bullets"** is versatile for most content slides
4. **Hide what you don't use** with `title showing` and `body showing` instead of leaving empty elements
5. **Don't change the master on slides with content** — custom text items remain, but defaults lose their formatting
6. **Use `default title item` and `default body item`** for title/body; add custom text items for extra content

---

## Sources

- [iWork Automation — Master Slides](https://iworkautomation.com/keynote/slide-masters.html)
- [iWork Automation — Default Text Items](https://iworkautomation.com/keynote/slide-default-text.html)
- [iWork Automation — Text Item Behavior](https://iworkautomation.com/keynote/text-item-behavior.html)
- [keynoteMP — slides.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/slides.ts)
- [keynoteMP — theme.ts](https://github.com/superdwayne/keynoteMP/blob/main/src/tools/theme.ts)
