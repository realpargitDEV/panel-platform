import { useMemo, useState } from 'react';

import { CATEGORIES, CATEGORY_BY_ID } from '../lib/themes/categories';
import { resolveTheme } from '../lib/themes/css';
import { THEMES } from '../lib/themes';
import { filterThemes, groupByCategory } from '../lib/themes/search';
import type { CategoryId, Theme } from '../lib/themes/types';
import Icon from '../ui/Icon';

/**
 * Choosing one of eighty-one themes.
 *
 * A flat grid of eighty-one cards is a list nobody scrolls to the end of, so
 * this is a browser rather than a gallery: a search box, the eight categories,
 * and results grouped under the heading they belong to.
 *
 * Every card renders a miniature of the real interface — canvas, sidebar, a
 * card, some text, the primary action — painted with that theme's *resolved*
 * tokens. Not a row of three swatches: swatches say what colours a theme
 * contains and nothing about what the application will look like in it, which
 * is the only question being asked here. Because the values come from
 * `resolveTheme`, a card cannot advertise a colour the theme does not use.
 */
export default function ThemeBrowser({
  value,
  onChange,
}: {
  value: string;
  onChange: (id: string) => void;
}) {
  const [text, setText] = useState('');
  const [category, setCategory] = useState<CategoryId | undefined>(undefined);

  const groups = useMemo(
    () => groupByCategory(filterThemes(THEMES, { text, category })),
    [text, category],
  );

  const total = useMemo(
    () => groups.reduce((sum, group) => sum + group.themes.length, 0),
    [groups],
  );

  return (
    <div className="flex flex-col gap-3 px-4 pb-4">
      <div className="flex flex-wrap items-center gap-2">
        <label className="relative flex min-w-[200px] flex-1 items-center">
          <span className="pointer-events-none absolute left-2.5 text-faint">
            <Icon name="search" size={13} />
          </span>
          <input
            type="search"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder={`Search ${THEMES.length} themes by name, colour or category`}
            aria-label="Search themes"
            className="h-9 w-full select-text rounded-[8px] border border-edge bg-raised pl-8 pr-3 text-[13px] text-ink placeholder:text-faint"
          />
        </label>

        <span className="tabular text-[12px] text-muted">
          {total} {total === 1 ? 'theme' : 'themes'}
        </span>
      </div>

      <div className="flex flex-wrap gap-1.5">
        <CategoryChip active={category === undefined} onClick={() => setCategory(undefined)}>
          All
        </CategoryChip>
        {CATEGORIES.map((entry) => (
          <CategoryChip
            key={entry.id}
            active={category === entry.id}
            onClick={() => setCategory(category === entry.id ? undefined : entry.id)}
          >
            {entry.name}
          </CategoryChip>
        ))}
      </div>

      {total === 0 && (
        <p className="py-8 text-center text-[13px] text-muted">
          No theme matches “{text}”. Try a colour, a mood, or a category name.
        </p>
      )}

      {groups.map((group) => {
        const meta = CATEGORY_BY_ID.get(group.category);
        return (
          <section key={group.category} className="flex flex-col gap-2">
            <header className="mt-2 flex flex-col gap-0.5">
              <h3 className="text-[13px] font-medium text-ink">{meta?.name}</h3>
              <p className="text-[12px] leading-snug text-muted">{meta?.detail}</p>
            </header>

            <div className="stagger grid gap-2.5 sm:grid-cols-2 xl:grid-cols-3">
              {group.themes.map((theme) => (
                <ThemeCard
                  key={theme.id}
                  theme={theme}
                  active={value === theme.id}
                  onSelect={() => onChange(theme.id)}
                />
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function CategoryChip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={`h-7 rounded-full border px-3 text-[12px] ${
        active
          ? 'border-accent bg-accent-soft text-ink'
          : 'border-edge bg-raised text-muted hover:text-ink'
      }`}
    >
      {children}
    </button>
  );
}

function ThemeCard({
  theme,
  active,
  onSelect,
}: {
  theme: Theme;
  active: boolean;
  onSelect: () => void;
}) {
  // The card previews the theme without applying it, so the values are read
  // directly rather than from a custom property that is still the current
  // theme's.
  const { colour, trait } = useMemo(() => resolveTheme(theme), [theme]);

  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onSelect}
      className={`flex flex-col gap-2 rounded-[10px] border p-2 text-left ${
        active ? 'border-accent bg-accent-soft' : 'border-edge bg-raised hover:border-edge-strong'
      }`}
    >
      <Miniature colour={colour} trait={trait} />

      <span className="flex min-w-0 flex-col gap-0.5 px-1 pb-0.5">
        <span className="flex items-center gap-1.5 text-[13px] font-medium text-ink">
          <span className="truncate">{theme.name}</span>
          {active && <Icon name="check" size={13} />}
        </span>
        <span className="line-clamp-2 text-[12px] leading-snug text-muted">{theme.detail}</span>

        {(theme.effect || theme.credit) && (
          <span className="mt-1 flex flex-wrap items-center gap-1">
            {theme.effect && (
              <span className="rounded-full border border-edge px-1.5 py-px text-[10px] uppercase tracking-wide text-faint">
                {theme.effect}
              </span>
            )}
            {theme.credit && (
              <span
                className="rounded-full border border-edge px-1.5 py-px text-[10px] text-faint"
                title={`${theme.credit.work} by ${theme.credit.author} — ${theme.credit.licence}`}
              >
                {theme.credit.licence === 'MIT' ? 'MIT' : 'credited'}
              </span>
            )}
          </span>
        )}
      </span>
    </button>
  );
}

/**
 * A 16:9 miniature of the application in this theme.
 *
 * Inline styles rather than classes throughout, because these are one theme's
 * values rendered while a different theme is on — the whole point is that the
 * card does not inherit the current palette.
 */
function Miniature({
  colour,
  trait,
}: {
  colour: Record<string, string>;
  trait: Record<string, string>;
}) {
  const radius = trait['radius-card'] ?? '6px';
  const border = trait['border-w'] ?? '1px';

  return (
    <span
      aria-hidden
      className="flex h-[86px] w-full overflow-hidden rounded-[8px]"
      style={{ background: colour.canvas, border: `1px solid ${colour.edge}` }}
    >
      {/* The rail, with its gradient and two nav rows. */}
      <span
        className="flex w-[30%] shrink-0 flex-col gap-1 p-1.5"
        style={{
          backgroundImage: `linear-gradient(180deg, ${colour['sidebar-top']}, ${colour['sidebar-bottom']})`,
          borderRight: `1px solid ${colour.edge}`,
        }}
      >
        <span
          className="h-3 w-3 shrink-0"
          style={{
            background: `linear-gradient(180deg, ${colour['brand-from']}, ${colour['brand-to']})`,
            borderRadius: radius,
          }}
        />
        <span className="mt-0.5 h-1 w-full" style={{ background: colour.ink, opacity: 0.75 }} />
        <span className="h-1 w-3/4" style={{ background: colour.muted, opacity: 0.6 }} />
        <span className="h-1 w-2/3" style={{ background: colour.muted, opacity: 0.35 }} />
      </span>

      {/* The page: a card, two lines of text, a primary action and a status. */}
      <span className="flex min-w-0 flex-1 flex-col gap-1 p-1.5">
        <span
          className="flex flex-1 flex-col justify-center gap-1 p-1.5"
          style={{
            background: colour.surface,
            border: `${border} solid ${colour.edge}`,
            borderRadius: radius,
          }}
        >
          <span className="h-1.5 w-2/3" style={{ background: colour.ink, opacity: 0.85 }} />
          <span className="h-1 w-full" style={{ background: colour.muted, opacity: 0.55 }} />
          <span className="h-1 w-1/2" style={{ background: colour.faint, opacity: 0.55 }} />
        </span>

        <span className="flex items-center gap-1">
          <span
            className="h-2.5 w-8"
            style={{
              background: `linear-gradient(180deg, ${colour['brand-from']}, ${colour['brand-to']})`,
              borderRadius: trait['radius-control'] ?? '4px',
            }}
          />
          <span className="h-1.5 w-1.5 rounded-full" style={{ background: colour.ok }} />
          <span className="h-1.5 w-1.5 rounded-full" style={{ background: colour.warn }} />
          <span className="h-1.5 w-1.5 rounded-full" style={{ background: colour.danger }} />
          <span className="ml-auto h-2 w-2 rounded-full" style={{ background: colour.accent }} />
        </span>
      </span>
    </span>
  );
}
