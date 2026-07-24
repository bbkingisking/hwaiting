import type { CardThemeId } from '@/lib/card-themes'

// Small pieces of theme-specific chrome that don't fit the "just override a
// CSS token" model (they add actual elements: tape, marquee lights, nails,
// scroll rods...). Kept out of flashcard.tsx so the component itself doesn't
// grow a per-theme branch for every visual gag — it just renders whatever
// these return, before/after the card.

export function CardThemeDecorationBefore({ themeId }: { themeId: CardThemeId }) {
  switch (themeId) {
    case 'arcade':
      return (
        <div className="ct-arcade-marquee">
          <div className="ct-arcade-bulbs">
            <span /><span /><span />
          </div>
          <span className="ct-arcade-title">HWAITING ARCADE</span>
          <div className="ct-arcade-bulbs">
            <span /><span /><span />
          </div>
        </div>
      )
    case 'ancientchina':
      return (
        <div className="ct-scroll-rod">
          <div className="ct-scroll-finial ct-scroll-finial-left" />
          <div className="ct-scroll-finial ct-scroll-finial-right" />
        </div>
      )
    default:
      return null
  }
}

export function CardThemeDecorationAfter({ themeId }: { themeId: CardThemeId }) {
  switch (themeId) {
    case 'postcard':
      return <div className="ct-postcard-tape" />
    case 'wildwest':
      return (
        <>
          <div className="ct-wildwest-nail ct-wildwest-nail-left" />
          <div className="ct-wildwest-nail ct-wildwest-nail-right" />
          <div className="ct-wildwest-star">NEW</div>
        </>
      )
    case 'ancientchina':
      return (
        <div className="ct-scroll-rod">
          <div className="ct-scroll-finial ct-scroll-finial-left" />
          <div className="ct-scroll-finial ct-scroll-finial-right" />
        </div>
      )
    case 'gameboy':
      return (
        <div className="ct-gameboy-label-strip">
          <span>HWAITING</span>
          <span>DOT MATRIX</span>
        </div>
      )
    default:
      return null
  }
}
