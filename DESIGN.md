---
name: Hopper
description: A technical field-manual system for native container operations.
colors:
  engine-navy: "#082852"
  deep-navy: "#041d3e"
  drafting-cyan: "#aff0f6"
  muted-cyan: "#77c3d4"
  manual-bone: "#f1f7f3"
  signal-vermilion: "#f74a3d"
  action-vermilion: "#d3342c"
  action-white: "#ffffff"
typography:
  display:
    fontFamily: "Barlow Condensed, sans-serif"
    fontSize: "clamp(52px, 7.2vw, 116px)"
    fontWeight: 700
    lineHeight: 0.84
  body:
    fontFamily: "Barlow Condensed, sans-serif"
    fontSize: "18px"
    fontWeight: 400
    lineHeight: 1.28
  label:
    fontFamily: "Barlow Condensed, sans-serif"
    fontSize: "13px"
    fontWeight: 700
    letterSpacing: "0.12em"
  docs-display:
    fontFamily: "Bricolage Grotesque, ui-sans-serif, sans-serif"
    fontWeight: 700
  docs-body:
    fontFamily: "Hanken Grotesk, ui-sans-serif, system-ui, sans-serif"
    fontWeight: 400
    lineHeight: 1.6
  docs-code:
    fontFamily: "JetBrains Mono, ui-monospace, monospace"
    fontWeight: 400
spacing:
  section-inline: "clamp(28px, 5vw, 84px)"
  section-block: "clamp(72px, 8vw, 132px)"
  rule-gap: "18px"
components:
  button-primary:
    backgroundColor: "{colors.action-vermilion}"
    textColor: "#ffffff"
    typography: "{typography.display}"
    rounded: "0"
    padding: "0.7rem 1.2rem"
  button-secondary:
    backgroundColor: "transparent"
    textColor: "{colors.manual-bone}"
    typography: "{typography.label}"
    rounded: "0"
    padding: "20px 0"
---

# Design System: Hopper

## Overview

**Creative North Star: "The Engine Field Manual"**

Hopper uses the visual language of a service manual drawn by the team that built the machine. Dense ruled fields, sectional diagrams, registration marks, and compressed industrial lettering make the product feel operational rather than promotional.

The system is flat, precise, and information-rich. Depth comes from technical illustration and overlapping evidence, not floating cards or soft shadows. Platform-specific facts may appear as current compatibility notes, but the core visual world describes Hopper as a container workbench that can expand to more hosts.

**Key Characteristics:**

- Saturated navy working field with bone and cyan drafting ink.
- Vermilion reserved for actions, sequence numbers, and registration marks.
- Condensed type, square controls, thin rules, and explicit operational labels.
- Sectional product proof instead of detached application screenshots.

## Colors

The palette combines dark blueprint stock with pale technical ink and a controlled safety-red signal.

### Primary

- **Engine Navy:** The continuous working field for the hero, capability ledger, and technical sections.
- **Deep Navy:** Footer and deepest separation layer.

### Secondary

- **Signal Vermilion:** Registration marks and sequence indicators.
- **Action Vermilion:** The darker accessible action surface for white button lettering.

### Tertiary

- **Drafting Cyan:** High-visibility rules, annotations, and secondary text.
- **Muted Cyan:** Quieter technical detail inside diagrams and supporting labels.

### Neutral

- **Manual Bone:** Primary copy and the inverted migration field.

**The Signal Discipline Rule.** Vermilion marks action or sequence; it never becomes general decoration.

## Typography

**Display Font:** Barlow Condensed (with sans-serif fallback)

**Body Font:** Barlow Condensed (with sans-serif fallback)

**Documentation Fonts:** Bricolage Grotesque for headings, Hanken Grotesk for prose, and JetBrains Mono for commands. The long-form reference keeps its established reading-oriented type system while the field-manual landing page uses the condensed family.

**Character:** A single condensed industrial family makes headlines, operational labels, and dense annotations feel drawn from the same manual. Scale, weight, spacing, and case establish hierarchy.

### Hierarchy

- **Display** (700, fluid 52–116px, 0.84): Section declarations and primary propositions.
- **Headline** (700, fluid 51–76px, 0.84): First-viewport promise.
- **Title** (700, 23–32px, 0.9): Procedure and ledger entries.
- **Body** (400, 18px, 1.4): Explanations, generally held below 66 characters per line.
- **Label** (700, 13px, 0.12em, uppercase): Registration strips, states, and table headers.

**The Compression Rule.** Display type may be tightly led and horizontally assertive; body copy must retain comfortable line height and readable measure.

## Layout

The desktop hero uses a measured 1536:1024 field with absolute regions derived from the approved composition. Later sections use asymmetric two-column grids, wide ruled ledgers, and generous block spacing. The 900px breakpoint converts every measured hero region into normal document flow, stacks columns, and preserves reading order. Section spacing uses the recorded inline and block tokens rather than a generic card grid.

## Elevation & Depth

The system uses no ambient shadows. Layers are communicated through contrast, rules, overlap, and the raster engine cutaway. A brief cyan scan line is the only luminous effect and is removed when reduced motion is requested.

**The Flat Instrument Rule.** Interface surfaces remain flat; illustration supplies material depth.

## Shapes

Corners are square. Controls use rules, rectangular frames, or a single clipped arrow point. Circular geometry is reserved for numbered callouts. Repeated diagonal registration strokes connect the wordmark, headings, and figure labels.

## Components

### Buttons

- **Shape:** Square field with an arrow-point right edge on the primary download control.
- **Primary:** Accessible action vermilion, white condensed uppercase lettering, and a download glyph.
- **Hover / Focus:** Small horizontal movement or brightness shift; focus uses an offset bone outline.
- **Secondary:** Transparent ruled rows with label typography and a directional arrow.

### Cards / Containers

- **Corner Style:** Square.
- **Background:** Alternating engine navy and manual bone section fields.
- **Shadow Strategy:** None.
- **Border:** One-pixel drafting rules define rows and boundaries.
- **Internal Padding:** Large section spacing, compact ledger-row spacing.

### Navigation

Navigation is uppercase, condensed, and embedded in the masthead rule. Hover shifts to drafting cyan. On mobile, secondary platform copy and the engine badge yield before the three primary destinations.

### Technical Ledger

Ledgers use ruled rows, a strong first column, pale-cyan operational detail, and a small vermilion or bone state. They replace interchangeable feature cards.

## Do's and Don'ts

### Do:

- **Do** organize capabilities as procedures, ledgers, matrices, and sectional diagrams.
- **Do** reserve vermilion for actions and sequence.
- **Do** keep current platform facts separate from the platform-neutral Hopper Engine story.
- **Do** provide normal-flow mobile layouts and reduced-motion behavior.

### Don't:

- **Don't** introduce rounded card grids, soft drop shadows, or generic gradient heroes.
- **Don't** place a detached product screenshot beside vague marketing copy.
- **Don't** use vermilion for long text or low-contrast decorative fills.
