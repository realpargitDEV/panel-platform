/**
 * File, Edit, Selection, View, Go, Run, Terminal, Help.
 *
 * Every entry runs a command the workspace already has; nothing here is a
 * placeholder. Where a menu would normally hold something this application does
 * not do, the entry is absent rather than present and dead — the exception
 * being items that are real but not available *right now* (save with nothing
 * open), which stay visible and disabled.
 */
import { useState } from 'react';

import ContextMenu, { type MenuEntry } from './ContextMenu';

export interface Menu {
  id: string;
  label: string;
  entries: MenuEntry[];
}

export default function MenuBar({ menus }: { menus: Menu[] }) {
  const [open, setOpen] = useState<{ id: string; x: number; y: number } | null>(null);

  function openMenu(id: string, element: HTMLElement) {
    const rect = element.getBoundingClientRect();
    setOpen({ id, x: rect.left, y: rect.bottom });
  }

  const entries = menus.find((menu) => menu.id === open?.id)?.entries;

  return (
    <div role="menubar" className="flex items-center">
      {menus.map((menu) => (
        <button
          key={menu.id}
          type="button"
          role="menuitem"
          aria-haspopup="menu"
          aria-expanded={open?.id === menu.id}
          onClick={(event) =>
            open?.id === menu.id ? setOpen(null) : openMenu(menu.id, event.currentTarget)
          }
          // Once one menu is open the others open on hover, the way a real
          // menu bar behaves — without this, browsing the menus means a click
          // to close and another to open.
          onMouseEnter={(event) => {
            if (open !== null) openMenu(menu.id, event.currentTarget);
          }}
          className={`h-full px-2 py-0.5 text-[13px] text-vs-text hover:bg-white/10 ${
            open?.id === menu.id ? 'bg-white/10' : ''
          }`}
        >
          {menu.label}
        </button>
      ))}

      {open && entries && (
        <ContextMenu
          entries={entries}
          position={{ x: open.x, y: open.y, below: true }}
          onClose={() => setOpen(null)}
        />
      )}
    </div>
  );
}
