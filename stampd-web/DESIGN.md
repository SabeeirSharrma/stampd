---
name: Stampd Dark
colors:
  surface: '#051424'
  surface-dim: '#051424'
  surface-bright: '#2c3a4c'
  surface-container-lowest: '#010f1f'
  surface-container-low: '#0d1c2d'
  surface-container: '#122131'
  surface-container-high: '#1c2b3c'
  surface-container-highest: '#273647'
  on-surface: '#d4e4fa'
  on-surface-variant: '#bdc8d1'
  inverse-surface: '#d4e4fa'
  inverse-on-surface: '#233143'
  outline: '#87929a'
  outline-variant: '#3e484f'
  surface-tint: '#7bd0ff'
  primary: '#8ed5ff'
  on-primary: '#00354a'
  primary-container: '#38bdf8'
  on-primary-container: '#004965'
  inverse-primary: '#00668a'
  secondary: '#bcc7de'
  on-secondary: '#263143'
  secondary-container: '#3e495d'
  on-secondary-container: '#aeb9d0'
  tertiary: '#c5cce6'
  on-tertiary: '#283044'
  tertiary-container: '#a9b1ca'
  on-tertiary-container: '#3c4459'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#c4e7ff'
  primary-fixed-dim: '#7bd0ff'
  on-primary-fixed: '#001e2c'
  on-primary-fixed-variant: '#004c69'
  secondary-fixed: '#d8e3fb'
  secondary-fixed-dim: '#bcc7de'
  on-secondary-fixed: '#111c2d'
  on-secondary-fixed-variant: '#3c475a'
  tertiary-fixed: '#dae2fd'
  tertiary-fixed-dim: '#bec6e0'
  on-tertiary-fixed: '#131b2e'
  on-tertiary-fixed-variant: '#3f465c'
  background: '#051424'
  on-background: '#d4e4fa'
  surface-variant: '#273647'
typography:
  headline-xl:
    fontFamily: Geist
    fontSize: 48px
    fontWeight: '600'
    lineHeight: '1.1'
    letterSpacing: -0.02em
  headline-lg:
    fontFamily: Geist
    fontSize: 32px
    fontWeight: '600'
    lineHeight: '1.2'
    letterSpacing: -0.01em
  headline-lg-mobile:
    fontFamily: Geist
    fontSize: 24px
    fontWeight: '600'
    lineHeight: '1.2'
  body-lg:
    fontFamily: Geist
    fontSize: 18px
    fontWeight: '400'
    lineHeight: '1.6'
  body-md:
    fontFamily: Geist
    fontSize: 16px
    fontWeight: '400'
    lineHeight: '1.5'
  label-md:
    fontFamily: Geist
    fontSize: 14px
    fontWeight: '500'
    lineHeight: '1.4'
    letterSpacing: 0.01em
  label-sm:
    fontFamily: Geist
    fontSize: 12px
    fontWeight: '500'
    lineHeight: '1.4'
    letterSpacing: 0.02em
rounded:
  sm: 0.25rem
  DEFAULT: 0.5rem
  md: 0.75rem
  lg: 1rem
  xl: 1.5rem
  full: 9999px
spacing:
  base: 4px
  xs: 4px
  sm: 8px
  md: 16px
  lg: 24px
  xl: 40px
  container-max: 1280px
  gutter: 24px
---

## Brand & Style

This design system translates the precision and minimalism of the Stampd aesthetic into a high-performance dark environment. The visual language is rooted in **Modern Minimalism** with a **Technical** edge, designed for high-focus environments where clarity and reduced eye strain are paramount. 

The emotional response should be one of sophisticated authority—quiet, confident, and meticulously organized. The interface leverages deep spatial depth and razor-sharp typography to create a premium digital experience that feels both architectural and lightweight.

## Colors

The palette is anchored by a deep-space foundation. The primary background uses a saturated Navy (#0f172a) to provide more visual interest than pure black. 

- **Primary Accent:** A luminous, high-vibrancy Blue (#38bdf8) derived from the Stampd Navy but shifted for optimal luminance on dark surfaces.
- **Surfaces:** Elevated containers use Charcoal (#1e293b) to create clear structural hierarchy.
- **Content:** Typography utilizes Off-White (#f8fafc) for maximum legibility, with Light Gray (#cbd5e1) for secondary metadata and icons to maintain visual balance.

## Typography

The design system exclusively utilizes **Geist**, a typeface engineered for precision and readability in technical interfaces. 

- **Headlines:** Use tight tracking and semi-bold weights to create a strong vertical rhythm. 
- **Body Text:** Maintains generous line-heights to ensure long-form readability against dark backgrounds, preventing "halation" effects where white text bleeds into the dark.
- **Labels:** Monospaced-adjacent qualities of Geist are leveraged for data points and UI labels to emphasize the systematic nature of the product.

## Layout & Spacing

This design system follows a strict **4px baseline grid**. Layouts are structured using a **12-column fluid grid** for desktop and a **4-column grid** for mobile devices.

- **Margins:** Large 40px (xl) margins on desktop to allow the UI to breathe. 
- **Internal Spacing:** Components use a 16px (md) standard padding for consistent internal balance.
- **Alignment:** All elements must align to the pixel grid to maintain the "sharp" technical aesthetic inherent to Geist.

## Elevation & Depth

In this dark-mode environment, depth is communicated through **Tonal Layering** rather than heavy shadows.

- **Level 0 (Base):** #0f172a (Primary background).
- **Level 1 (Card/Surface):** #1e293b (Secondary color).
- **Level 2 (Popovers/Modals):** #334155 (A lighter charcoal tint) with a subtle 1px border of #475569 to define edges.
- **Accents:** High-vibrancy glows are used sparingly. When an element is focused, use a subtle outer glow utilizing the primary accent color at 20% opacity.

## Shapes

The shape language is controlled and modern. We utilize a **Rounded (Level 2)** approach to soften the technical nature of the typography.

- **Standard Elements:** 0.5rem (8px) radius for buttons and input fields.
- **Large Containers:** 1rem (16px) for cards and modals to create a distinct container identity.
- **Pills:** Full-round radius is reserved specifically for status tags and interactive chips.

## Components

### Buttons
- **Primary:** Solid #38bdf8 with #0f172a text. No gradient. High contrast.
- **Secondary:** Transparent background with a 1px border of #cbd5e1. Text in #f8fafc.
- **Ghost:** No background or border. Text in #cbd5e1, shifting to #f8fafc on hover.

### Input Fields
- **Default State:** Background #1e293b with a 1px border of #334155.
- **Focus State:** Border shifts to #38bdf8 with a subtle 2px outer ring.
- **Placeholder:** Text color #64748b (muted navy-gray).

### Cards
- Surfaces use #1e293b. Borders are minimal (1px #334155). 
- Hover states for interactive cards should slightly lighten the background color to #334155 rather than increasing shadow.

### Lists & Navigation
- Active states in navigation should use a vertical 2px "indicator" line of #38bdf8 on the left or bottom edge, combined with a subtle #f8fafc text color shift.