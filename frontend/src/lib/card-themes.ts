// Registry of selectable flashcard skins. Each theme is just data: a set of
// CSS custom-property overrides (scoped under [data-card-theme]) plus a couple
// of label strings. The actual visuals live in styles/card-themes.css; adding
// a theme here + a matching CSS block is all that's needed to make a new one
// selectable, no changes to flashcard.tsx required.

export type CardThemeId =
  | 'default'
  | 'postcard'
  | 'arcade'
  | 'neumorphic'
  | 'gameboy'
  | 'chalkboard'
  | 'wildwest'
  | 'ancientchina'

interface CardThemeDef {
  id: CardThemeId
  name: string
  checkLabel: string
  newLabel: string
  swatch: { bg: string; fg: string; accent: string }
}

export const CARD_THEMES: CardThemeDef[] = [
  {
    id: 'default',
    name: 'Default',
    checkLabel: 'Check',
    newLabel: 'New',
    swatch: { bg: '#ffffff', fg: '#171717', accent: '#171717' },
  },
  {
    id: 'postcard',
    name: 'Postcard',
    checkLabel: 'CHECK',
    newLabel: 'NEW',
    swatch: { bg: '#fbf7ec', fg: '#3d3226', accent: '#b23b2e' },
  },
  {
    id: 'arcade',
    name: 'Arcade marquee',
    checkLabel: '★ Insert Answer ★',
    newLabel: 'NEW',
    swatch: { bg: '#150c24', fg: '#ffffff', accent: '#ff2d95' },
  },
  {
    id: 'neumorphic',
    name: 'Neumorphic',
    checkLabel: 'Check',
    newLabel: 'New',
    swatch: { bg: '#e8ebf0', fg: '#57606f', accent: '#8a94a6' },
  },
  {
    id: 'gameboy',
    name: 'Pixel / Game Boy',
    checkLabel: '► CHECK',
    newLabel: 'NEW',
    swatch: { bg: '#9bbc0f', fg: '#0f380f', accent: '#306230' },
  },
  {
    id: 'chalkboard',
    name: 'Chalkboard',
    checkLabel: 'check ✓',
    newLabel: 'new',
    swatch: { bg: '#2f3e35', fg: '#eef0ea', accent: '#6b4a30' },
  },
  {
    id: 'wildwest',
    name: 'Wild West',
    checkLabel: 'Draw! (Check)',
    newLabel: 'NEW',
    swatch: { bg: '#e8d5ab', fg: '#3a2a1c', accent: '#6b4a2b' },
  },
  {
    id: 'ancientchina',
    name: 'Ancient China',
    checkLabel: '確認 Check',
    newLabel: 'New',
    swatch: { bg: '#f3e3c0', fg: '#3d2814', accent: '#a3231a' },
  },
]

export const DEFAULT_CARD_THEME: CardThemeId = 'default'

export function getCardTheme(id: CardThemeId): CardThemeDef {
  return CARD_THEMES.find((t) => t.id === id) ?? CARD_THEMES[0]
}
