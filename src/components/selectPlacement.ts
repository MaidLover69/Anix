export const MENU_GAP = 4;
export const MENU_MARGIN = 8;

export interface PlacementInput {
  trigger: { top: number; bottom: number; left: number; width: number };
  menuWidth: number;
  menuHeight: number;
  viewportWidth: number;
  viewportHeight: number;
  preferred?: 'up' | 'down';
  /** Which edge of the trigger the menu lines up with; 'right' makes a wide menu grow leftwards. */
  align?: 'left' | 'right';
}

export interface Placement {
  side: 'up' | 'down';
  top: number;
  left: number;
  maxHeight: number;
}

/** Where a menu opens: the preferred side when it fits, else the other, else the larger (shortened, it scrolls). */
export function menuPlacement(input: PlacementInput): Placement {
  const { trigger, menuWidth, menuHeight, viewportWidth, viewportHeight, preferred = 'down', align = 'left' } = input;
  const below = viewportHeight - trigger.bottom - MENU_GAP - MENU_MARGIN;
  const above = trigger.top - MENU_GAP - MENU_MARGIN;
  const room = { down: below, up: above };
  const other = preferred === 'down' ? 'up' : 'down';
  const side =
    room[preferred] >= menuHeight ? preferred : room[other] >= menuHeight ? other : below >= above ? 'down' : 'up';
  const maxHeight = Math.min(menuHeight, Math.max(room[side], 0));
  const top = side === 'down' ? trigger.bottom + MENU_GAP : trigger.top - MENU_GAP - maxHeight;
  const wanted = align === 'right' ? trigger.left + trigger.width - menuWidth : trigger.left;
  const left = Math.max(MENU_MARGIN, Math.min(wanted, viewportWidth - menuWidth - MENU_MARGIN));
  return { side, top, left, maxHeight };
}
