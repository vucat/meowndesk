import type { View } from '../telemetry/types';
import { formatBytes } from '../telemetry/format';
import { BentoCard } from './BentoCard';

interface AppTile {
  name: string;
  bytes: bigint;
  count: number;
}

const PALETTE = [
  ['#3b82f6', '#1d4ed8'],
  ['#818cf8', '#4f46e5'],
  ['#fbbf24', '#b45309'],
  ['#34d399', '#047857'],
  ['#fb7185', '#be123c'],
  ['#c084fc', '#7e22ce'],
  ['#22d3ee', '#0e7490'],
] as const;

type Glyph = 'browser' | 'terminal' | 'code' | 'ai' | 'docker' | 'system';

function AppBadge({ name }: { name: string }) {
  const normalized = name.toLowerCase().replace(/\.exe$/, '');
  let glyph: Glyph | null = null;
  let accent =
    [...normalized].reduce((hash, letter) => (hash * 31 + (letter.codePointAt(0) ?? 0)) >>> 0, 0) %
    PALETTE.length;
  if (/chrome|edge|firefox|safari|browser|brave|chromium/.test(normalized)) {
    glyph = 'browser';
    accent = 0;
  } else if (/wsl|vmmem|terminal|powershell|bash|zsh/.test(normalized)) {
    glyph = 'terminal';
    accent = 2;
  } else if (/code|cursor|idea|studio|xcode/.test(normalized)) {
    glyph = 'code';
    accent = normalized.includes('opencode') ? 5 : 1;
  } else if (/ollama|chatgpt|claude|openai/.test(normalized)) {
    glyph = 'ai';
    accent = 3;
  } else if (/docker|container/.test(normalized)) {
    glyph = 'docker';
    accent = 6;
  } else if (/system|explorer|kernel|service/.test(normalized)) {
    glyph = 'system';
    accent = 4;
  }
  const colors = PALETTE[accent] ?? PALETTE[0];
  const words = name.trim().split(/[\s._-]+/);
  const monogram = (
    words.length > 1
      ? words
          .slice(0, 2)
          .map((word) => [...word][0])
          .join('')
      : [...name].slice(0, 2).join('')
  ).toLocaleUpperCase();
  return (
    <span
      className="treemap-badge rounded-lg"
      style={{ background: `linear-gradient(145deg, ${colors[0]}, ${colors[1]})` }}
    >
      {glyph ? (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.7"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          focusable="false"
        >
          {glyph === 'browser' && (
            <>
              <circle cx="12" cy="12" r="8" />
              <ellipse cx="12" cy="12" rx="3.5" ry="8" />
              <path d="M4 12h16M6 7.5h12M6 16.5h12" />
            </>
          )}
          {glyph === 'terminal' && (
            <>
              <rect x="3" y="4" width="18" height="16" rx="3" />
              <path d="m7 9 3 3-3 3m6 0h4" />
            </>
          )}
          {glyph === 'code' && (
            <>
              <path d="m8 7-5 5 5 5m8-10 5 5-5 5M14 4l-4 16" />
            </>
          )}
          {glyph === 'ai' && (
            <>
              <path d="m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3Z" />
              <path d="m19 3 .5 1.5L21 5l-1.5.5L19 7l-.5-1.5L17 5l1.5-.5L19 3Z" />
            </>
          )}
          {glyph === 'docker' && (
            <>
              <path d="M3 12h14c1.5 0 2.5-.6 3-2l1 2c-1 1-2 1.5-3 1.5C17 18 14 20 9 20c-3.5 0-6-2.5-6-8Z" />
              <path d="M5 8h4v4H5zm4 0h4v4H9zm4 0h4v4h-4zM9 4h4v4H9z" />
            </>
          )}
          {glyph === 'system' && (
            <>
              <rect x="6" y="6" width="12" height="12" rx="3" />
              <path d="M10 10h4v4h-4zM9 3v3m6-3v3M9 18v3m6-3v3M3 9h3m-3 6h3m12-6h3m-3 6h3" />
            </>
          )}
        </svg>
      ) : (
        monogram
      )}
    </span>
  );
}

function tooltipMemory(bytes: bigint): string {
  const gb = 1_073_741_824n;
  if (bytes < gb || bytes >= 1_099_511_627_776n) return formatBytes(bytes);
  // Round for display only; retain exact BigInt totals and ordering.
  const hundredths = (bytes * 100n + gb / 2n) / gb;
  return `${hundredths / 100n}.${(hundredths % 100n).toString().padStart(2, '0')} GB`;
}

function Tile({ tile, compact = false }: { tile: AppTile; compact?: boolean }) {
  return (
    <div
      className={`treemap-block rounded-xl${compact ? ' treemap-compact' : ''}`}
      title={`${tile.name} · ${tooltipMemory(tile.bytes)}${tile.count > 1 ? ` · ${tile.count} processes` : ''}`}
    >
      <AppBadge name={tile.name} />
      {!compact && (
        <div className="treemap-label">
          <span className="treemap-name">{tile.name}</span>
          <span className="treemap-value">{formatBytes(tile.bytes)}</span>
        </div>
      )}
    </div>
  );
}

export function MemoryAppCard({ view }: { view: View | null }) {
  const summaries = view?.processSummaries;
  const entries = summaries?.memoryTop ?? [];
  // Defensive exact-name grouping also supports older/test snapshots; Rust ranks whole apps.
  const apps = new Map<string, AppTile>();
  for (const process of entries) {
    const bytes = BigInt(process.residentBytes.value ?? '0');
    if (bytes === 0n) continue;
    const app = apps.get(process.name);
    if (app) {
      app.bytes += bytes;
      app.count += 1;
    } else {
      apps.set(process.name, { name: process.name, bytes, count: 1 });
    }
  }
  const tiles = [...apps.values()].sort((a, b) =>
    a.bytes === b.bytes ? a.name.localeCompare(b.name) : a.bytes > b.bytes ? -1 : 1,
  );
  const hero = tiles[0];
  const middle = tiles.slice(1, 3);
  const compact = tiles.slice(3, 7);
  const caveat = 'Shared pages may be counted more than once.';
  return (
    <BentoCard
      title="Memory by App"
      resource="memory"
      className="summary-card memory-app-card"
      badge={
        <span className="memory-app-total" title={caveat}>
          <span className="sr-only">Displayed total </span>
          {formatBytes(summaries?.displayedMemoryTotalBytes ?? '0')} in all
        </span>
      }
    >
      {hero ? (
        <div
          className="memory-app-bento gap-2"
          data-columns={compact.length ? 3 : middle.length ? 2 : 1}
          aria-hidden="true"
        >
          <Tile tile={hero} />
          {middle.length > 0 && (
            <div className="memory-app-middle gap-2" data-count={middle.length}>
              {middle.map((tile) => (
                <Tile key={tile.name} tile={tile} />
              ))}
            </div>
          )}
          {compact.length > 0 && (
            <div className="memory-app-icons gap-2">
              {compact.map((tile) => (
                <Tile key={tile.name} tile={tile} compact />
              ))}
            </div>
          )}
        </div>
      ) : (
        <p className="summary-empty">No readable memory observations</p>
      )}
      {entries.length > 0 && (
        <ul className="sr-only">
          {entries.map((process) => (
            <li key={process.key}>
              <span>{process.name}</span>{' '}
              <span>
                {process.residentBytes.value === null
                  ? '—'
                  : formatBytes(process.residentBytes.value)}
              </span>
            </li>
          ))}
        </ul>
      )}
      <span className="sr-only">{caveat}</span>
    </BentoCard>
  );
}
