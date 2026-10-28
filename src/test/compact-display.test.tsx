import { createElement, type ComponentType } from 'react';
import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import { App } from '../App';
import { TopProcessesCard } from '../components/TopProcessesCard';
import {
  capacityFreeFill,
  percentFill,
  powerFill,
  scaleFill,
  sharePercent,
} from '../telemetry/display';
import { formatBytes, formatPercent } from '../telemetry/format';
import type { Reading } from '../telemetry/types';
import { validateView } from '../telemetry/validate';
import { missing } from '../telemetry/selectors';
import { MockTransport } from './mock-transport';

const coreCards = ['CPU', 'Memory', 'GPU & VRAM', 'Disk', 'Network', 'Power/Thermals'];

function mount(scenario = 'full') {
  const transport = new MockTransport(scenario);
  render(createElement(App as ComponentType<{ transport: MockTransport }>, { transport }));
  return transport;
}

function valid<T>(value: T): Reading<T> {
  return {
    status: 'valid',
    value,
    sampledAtMs: 1_761_091_199_000,
    elapsedMs: 63_000,
    scopeId: 'test:scope',
    reason: null,
  };
}

it('defaults to the Charts display mode with the range selector and six canvases', async () => {
  mount();
  const mode = await screen.findByRole('radiogroup', { name: 'Display mode' });
  expect(within(mode).getByRole('radio', { name: 'Charts' })).toBeChecked();
  expect(within(mode).getByRole('radio', { name: 'Compact' })).not.toBeChecked();
  expect(screen.getByRole('radiogroup', { name: 'Time range' })).toBeVisible();
  expect(document.querySelectorAll('.dot-chart canvas')).toHaveLength(6);
  expect(screen.getAllByRole('region')).toHaveLength(9);
  expect(screen.queryByRole('region', { name: 'Top Processes' })).not.toBeInTheDocument();
});

it('hides the range selector, unmounts every chart and adds one micro-meter per core card', async () => {
  const transport = mount();
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  await userEvent.click(screen.getByRole('radio', { name: 'Compact' }));

  expect(screen.queryByRole('radiogroup', { name: 'Time range' })).not.toBeInTheDocument();
  expect(document.querySelectorAll('.dot-chart canvas')).toHaveLength(0);
  expect(screen.queryAllByRole('figure')).toHaveLength(0);
  expect(screen.getByRole('button', { name: 'Pause' })).toBeVisible();

  const sample = transport.currentView.sample!;
  for (const name of coreCards) {
    const card = screen.getByRole('region', { name });
    expect(card).toHaveClass('bento-card-compact');
    const meters = card.querySelectorAll('.micro-meter');
    expect(meters).toHaveLength(1);
    expect(meters[0]!.querySelector('.micro-meter-fill')).not.toBeNull();
    const stats = card.querySelector('.secondary-stats');
    expect(stats).not.toBeNull();
    expect(stats!.querySelectorAll('.secondary-stat')).toHaveLength(3);
    expect(card.querySelector('.primary-label')).not.toBeNull();
    expect(card.querySelector('.resource-symbol svg')).not.toBeNull();
  }
  const meterFill = (name: string) =>
    screen.getByRole('region', { name }).querySelector('.micro-meter')!.getAttribute('data-fill');
  expect(meterFill('CPU')).toBe(String(sample.cpu.totalPct.value));
  expect(meterFill('Memory')).toBe(String(sample.memory.usedPct.value));
  expect(meterFill('GPU & VRAM')).toBe(String(sample.gpu.utilizationPct.value));
  expect(meterFill('Disk')).toBe(
    String(
      Number(
        (BigInt(sample.disk.freeBytes.value!) * 1000n) / BigInt(sample.disk.totalBytes.value!),
      ) / 10,
    ),
  );
  expect(meterFill('Power/Thermals')).toBe(String(sample.power.primaryValue.value));
});

it('labels each compact card with its scope or memory badge in the heading slot', async () => {
  mount();
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  await userEvent.click(screen.getByRole('radio', { name: 'Compact' }));
  const scope = (name: string) => screen.getByRole('region', { name }).querySelector('.card-scope');
  expect(scope('CPU')).toHaveTextContent('CPU package');
  expect(scope('GPU & VRAM')).toHaveTextContent('NVIDIA GeForce RTX 3060');
  expect(scope('Disk')).toHaveTextContent('/ (ext4)');
  expect(scope('Network')).toHaveTextContent('Interface traffic');
  expect(scope('Power/Thermals')).toHaveTextContent('BAT0');
  const memory = screen.getByRole('region', { name: 'Memory' });
  expect(within(memory).getByText(/Normal/)).toBeVisible();
  expect(memory.querySelector('.card-scope')).toBeNull();
});

it('replaces the three breakdown cards with one unified Top Processes strip', async () => {
  mount();
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  await userEvent.click(screen.getByRole('radio', { name: 'Compact' }));
  const strip = screen.getByRole('region', { name: 'Top Processes' });
  for (const name of ['Memory by Type', 'Memory by App', 'Resource Load'])
    expect(screen.queryByRole('region', { name })).not.toBeInTheDocument();
  for (const name of coreCards) expect(screen.getByRole('region', { name })).toBeVisible();

  const view = new MockTransport('full').currentView;
  const entries = view.processSummaries!.memoryTop;
  const rows = strip.querySelectorAll('.top-process-item');
  expect(rows).toHaveLength(entries.length);
  const stripList = strip.querySelector('.top-processes-strip') as HTMLElement;
  expect(stripList.style.gridTemplateColumns).toBe(`repeat(${entries.length}, minmax(0, 1fr))`);
  expect(strip).toHaveTextContent(/Combined Memory & CPU Load/);
  expect(within(strip).getByText('Sorted by memory')).toBeVisible();
  const total = entries.reduce((sum, entry) => sum + BigInt(entry.residentBytes.value!), 0n);
  const first = rows[0]!;
  expect(first.querySelector('.top-process-name')).toHaveTextContent(entries[0]!.name);
  expect(first.querySelector('.top-process-cpu')).toHaveTextContent(
    formatPercent(entries[0]!.cpuPct.value!),
  );
  expect(first.querySelector('.top-process-ram')).toHaveTextContent(
    formatBytes(entries[0]!.residentBytes.value!),
  );
  expect(first.querySelector('.top-process-share')).toHaveTextContent(
    formatPercent(sharePercent(BigInt(entries[0]!.residentBytes.value!), total)!),
  );
  const bar = first.querySelector('.top-process-bar');
  expect(bar).not.toBeNull();
  expect((bar as HTMLElement).style.width).toBe(
    `${sharePercent(BigInt(entries[0]!.residentBytes.value!), total)}%`,
  );
  expect(strip).toHaveTextContent(/shared pages/i);
});

it('labels the eight-app memory-list total without attributing it to five columns', async () => {
  const transport = mount('memory_bento');
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  await userEvent.click(screen.getByRole('radio', { name: 'Compact' }));
  const strip = screen.getByRole('region', { name: 'Top Processes' });
  const all = transport.currentView.processSummaries!.memoryTop;
  expect(all.length).toBe(8);
  const total = BigInt(transport.currentView.processSummaries!.displayedMemoryTotalBytes);
  expect(strip.querySelectorAll('.top-process-item')).toHaveLength(5);
  expect(strip.querySelector('.top-processes-total')).toHaveTextContent(formatBytes(total));
  expect(strip).toHaveTextContent('Memory list total');
  expect(strip).not.toHaveTextContent('across 5 active apps');
  for (const [index, row] of [...strip.querySelectorAll('.top-process-item')].entries()) {
    const share = sharePercent(BigInt(all[index]!.residentBytes.value!), total)!;
    expect(row.querySelector('.top-process-share')).toHaveTextContent(formatPercent(share));
    expect((row.querySelector('.top-process-bar') as HTMLElement).style.width).toBe(`${share}%`);
  }
});

it.each(['unavailable', 'stale'] as const)(
  'shows unavailable memory totals and shares for CPU fallback with %s resident readings',
  (status) => {
    const view = new MockTransport('full').currentView;
    const summaries = view.processSummaries!;
    summaries.memoryTop = [];
    summaries.displayedMemoryTotalBytes = '0';
    for (const entry of summaries.cpuTop) {
      entry.residentBytes = {
        ...entry.residentBytes,
        status,
        value: status === 'stale' ? entry.residentBytes.value : null,
        sampledAtMs: status === 'stale' ? entry.residentBytes.sampledAtMs : null,
        elapsedMs: status === 'stale' ? entry.residentBytes.elapsedMs : null,
        reason: 'No current resident-memory observation',
      };
    }
    render(<TopProcessesCard view={validateView(view)} />);
    const strip = screen.getByRole('region', { name: 'Top Processes' });
    expect(strip).toHaveTextContent('Sorted by CPU');
    expect(strip.querySelector('.top-processes-total')).toHaveTextContent('—');
    expect(strip.querySelector('.top-processes-total')).not.toHaveTextContent('0 B');
    for (const row of strip.querySelectorAll('.top-process-item')) {
      expect(row.querySelector('.top-process-ram')).toHaveTextContent(
        status === 'stale' ? 'Last valid' : '—',
      );
      if (status === 'stale') expect(row.querySelector('.top-process-ram time')).not.toBeNull();
      expect(row.querySelector('.top-process-share')).toHaveTextContent('—');
      expect((row.querySelector('.top-process-bar') as HTMLElement).style.width).toBe('0%');
      expect(row.querySelector('.top-process-cpu')).toHaveTextContent(/%/);
    }
  },
);

it('preserves measured zero resident memory without manufacturing a percentage', () => {
  const view = new MockTransport('full').currentView;
  const summaries = view.processSummaries!;
  summaries.memoryTop = [summaries.memoryTop[0]!];
  summaries.memoryTop[0]!.residentBytes.value = '0';
  summaries.displayedMemoryTotalBytes = '0';
  render(<TopProcessesCard view={validateView(view)} />);
  const strip = screen.getByRole('region', { name: 'Top Processes' });
  expect(strip.querySelector('.top-processes-total')).toHaveTextContent('0 B');
  expect(strip.querySelector('.top-process-ram')).toHaveTextContent('0 B');
  expect(strip.querySelector('.top-process-share')).toHaveTextContent('—');
  expect((strip.querySelector('.top-process-bar') as HTMLElement).style.width).toBe('0%');
});

it('switches back to Charts with the full nine-card layout and range selector', async () => {
  mount();
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  const compact = screen.getByRole('radio', { name: 'Compact' });
  compact.focus();
  await userEvent.keyboard(' ');
  expect(compact).toBeChecked();
  expect(screen.queryByRole('radiogroup', { name: 'Time range' })).not.toBeInTheDocument();

  const charts = screen.getByRole('radio', { name: 'Charts' });
  charts.focus();
  await userEvent.keyboard(' ');
  expect(charts).toBeChecked();
  expect(screen.getAllByRole('region')).toHaveLength(9);
  expect(screen.getByRole('radiogroup', { name: 'Time range' })).toBeVisible();
  expect(document.querySelectorAll('.dot-chart canvas')).toHaveLength(6);
  expect(screen.queryByRole('region', { name: 'Top Processes' })).not.toBeInTheDocument();
});

it('preserves paused 15m inspection and issues no controls across Charts → Compact → Charts', async () => {
  const transport = mount();
  const commands = [
    vi.spyOn(transport, 'setView'),
    vi.spyOn(transport, 'setPaused'),
    vi.spyOn(transport, 'setVisible'),
    vi.spyOn(transport, 'setPreferences'),
  ];
  try {
    await screen.findByRole('radiogroup', { name: 'Display mode' });
    await userEvent.click(screen.getByRole('radio', { name: '15m' }));
    await userEvent.click(screen.getByRole('button', { name: 'Pause' }));
    expect(screen.getByRole('button', { name: 'Resume' })).toHaveAttribute('aria-pressed', 'true');
    const frozen = transport.currentView;
    expect(frozen.selection.range).toBe('15m');
    expect(frozen.mode).toBe('paused');
    const endpointText = screen.getByTestId('history-status').textContent;
    expect(endpointText).toBe(`Frozen at ${new Date(frozen.endpointAtMs).toLocaleTimeString()}`);
    const cpuText = screen.getByTestId('cpu-summary').textContent;
    const calls = commands.map((command) => command.mock.calls.length);

    for (const mode of ['Compact', 'Charts']) {
      await userEvent.click(screen.getByRole('radio', { name: mode }));
      act(() => transport.tick());
      expect(commands.map((command) => command.mock.calls.length)).toEqual(calls);
      expect(transport.currentView.revision).toBe(frozen.revision);
      expect(transport.currentView.selection).toEqual(frozen.selection);
      expect(transport.currentView.inspectionId).toBe(frozen.inspectionId);
      expect(screen.getByTestId('history-status').textContent).toBe(endpointText);
      expect(screen.getByTestId('cpu-summary').textContent).toBe(cpuText);
      expect(screen.getByRole('button', { name: 'Resume' })).toHaveAttribute(
        'aria-pressed',
        'true',
      );
      expect(screen.queryByText('Live updates interrupted')).not.toBeInTheDocument();
    }
    // Collection continued behind the frozen presentation.
    expect(transport.currentView.endpointElapsedMs).toBeGreaterThan(frozen.endpointElapsedMs);
    expect(screen.getByRole('radio', { name: '15m' })).toBeChecked();
    expect(screen.getAllByRole('region')).toHaveLength(9);
    expect(document.querySelectorAll('.dot-chart canvas')).toHaveLength(6);
    expect(screen.getByRole('radiogroup', { name: 'Time range' })).toBeVisible();
    for (const chart of screen.getAllByRole('figure'))
      expect(within(chart).getByText(/^15m · /)).toBeVisible();
  } finally {
    commands.forEach((command) => command.mockRestore());
  }
});

it('retains an existing live freshness warning across display changes', async () => {
  const now = vi.spyOn(performance, 'now').mockReturnValue(0);
  const transport = mount('stopped_delivery');
  try {
    await screen.findByRole('radiogroup', { name: 'Display mode' });
    now.mockReturnValue(2001);
    await act(async () => document.dispatchEvent(new Event('visibilitychange')));
    expect(screen.getByText('Live updates interrupted')).toBeVisible();
    const before = transport.currentView;
    for (const mode of ['Compact', 'Charts']) {
      await userEvent.click(screen.getByRole('radio', { name: mode }));
      expect(screen.getByText('Live updates interrupted')).toBeVisible();
      expect(transport.currentView).toEqual(before);
    }
  } finally {
    now.mockRestore();
  }
});

it('marks retained Compact process values and meters stale until an advancing sample recovers', async () => {
  const now = vi.spyOn(performance, 'now').mockReturnValue(0);
  const transport = mount('full');
  try {
    await screen.findByRole('radiogroup', { name: 'Display mode' });
    await userEvent.click(screen.getByRole('radio', { name: 'Compact' }));
    const strip = screen.getByRole('region', { name: 'Top Processes' });
    const retained = transport.currentView;
    const total = formatBytes(retained.processSummaries!.displayedMemoryTotalBytes);
    now.mockReturnValue(2001);
    await act(async () => document.dispatchEvent(new Event('visibilitychange')));
    expect(screen.getByText('Live updates interrupted')).toBeVisible();
    expect(strip.querySelector('.top-processes-total')).toHaveTextContent(total);
    for (const selector of [
      '.top-processes-total',
      '.top-process-cpu',
      '.top-process-ram',
      '.top-process-share',
    ]) {
      for (const value of strip.querySelectorAll(selector)) {
        expect(value.querySelector('.metric-stale')).not.toBeNull();
        expect(value).toHaveTextContent('Last valid');
        expect(value.querySelector('time')).not.toBeNull();
      }
    }
    for (const meter of document.querySelectorAll('.micro-meter'))
      expect(meter).toHaveAttribute('data-fill', '0');
    for (const bar of strip.querySelectorAll('.top-process-bar'))
      expect((bar as HTMLElement).style.width).toBe('0%');
    expect(transport.currentView).toEqual(retained);
    act(() => transport.tick());
    expect(screen.queryByText('Live updates interrupted')).not.toBeInTheDocument();
    expect(strip.querySelector('.metric-stale')).toBeNull();
    expect(strip.querySelector('.top-processes-total')).toHaveTextContent(total);
    expect(
      screen.getByRole('region', { name: 'CPU' }).querySelector('.micro-meter'),
    ).not.toHaveAttribute('data-fill', '0');
    expect(
      parseFloat((strip.querySelector('.top-process-bar') as HTMLElement).style.width),
    ).toBeGreaterThan(0);
  } finally {
    now.mockRestore();
  }
});

it.each(['unsupported', 'permission_denied', 'unavailable'] as const)(
  'preserves explicit %s CPU state during Compact interruption',
  (status) => {
    const view = new MockTransport('full').currentView;
    view.processSummaries!.cpuTop = [];
    view.processSummaries!.memoryTop[0]!.cpuPct = {
      ...missing<number>('Process CPU cannot be read'),
      status,
    };
    render(<TopProcessesCard view={validateView(view)} interrupted />);
    const cpu = screen
      .getByRole('region', { name: 'Top Processes' })
      .querySelector('.top-process-cpu')!;
    expect(cpu).toHaveTextContent('—');
    expect(cpu).toHaveTextContent('Process CPU cannot be read');
    expect(cpu.querySelector('.metric-stale')).toBeNull();
    expect(cpu).not.toHaveTextContent('Last valid');
  },
);

it('resets the session-local display mode to Charts on a fresh mount', async () => {
  const first = render(<App transport={new MockTransport('full')} />);
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  await userEvent.click(screen.getByRole('radio', { name: 'Compact' }));
  expect(screen.getByRole('radio', { name: 'Compact' })).toBeChecked();
  first.unmount();
  render(<App transport={new MockTransport('full')} />);
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  expect(screen.getByRole('radio', { name: 'Charts' })).toBeChecked();
  expect(screen.getByRole('radiogroup', { name: 'Time range' })).toBeVisible();
  expect(screen.getAllByRole('region')).toHaveLength(9);
  expect(document.querySelectorAll('.dot-chart canvas')).toHaveLength(6);
});

it('keeps the six core cards and an accessible empty strip without process observations', async () => {
  mount('initializing');
  await screen.findByRole('radiogroup', { name: 'Display mode' });
  await userEvent.click(screen.getByRole('radio', { name: 'Compact' }));
  for (const name of coreCards) expect(screen.getByRole('region', { name })).toBeVisible();
  const strip = screen.getByRole('region', { name: 'Top Processes' });
  expect(strip).toHaveTextContent('No readable process observations');
  expect(strip.querySelectorAll('.top-process-item')).toHaveLength(0);
});

it('derives BigInt-safe meter fills and rejects non-valid readings', () => {
  expect(capacityFreeFill(valid('412316860416'), valid('1000204886016'))).toBe(41.2);
  expect(capacityFreeFill(valid('5'), valid('0'))).toBe(0);
  expect(capacityFreeFill(valid('5'), undefined)).toBe(0);
  expect(capacityFreeFill({ ...valid('5'), status: 'stale' }, valid('10'))).toBe(0);
  expect(capacityFreeFill(valid('not-bytes'), valid('10'))).toBe(0);
  expect(percentFill(valid(42.5))).toBe(42.5);
  expect(percentFill(valid(140))).toBe(100);
  expect(percentFill(valid(-3))).toBe(0);
  expect(percentFill({ ...valid(42.5), status: 'stale' })).toBe(0);
  expect(percentFill(undefined)).toBe(0);
  expect(scaleFill(valid(2_000_000), 4_000_000)).toBe(50);
  expect(scaleFill(valid(2), 0)).toBe(0);
  expect(scaleFill(valid(2), undefined)).toBe(0);
  expect(scaleFill({ ...valid(2), status: 'unavailable' }, 4)).toBe(0);
  expect(sharePercent(1048576000n, 1782579200n)).toBe(58.8);
  expect(sharePercent(5n, 0n)).toBeNull();
  expect(powerFill(new MockTransport('full').currentView)).toBe(67);
});
