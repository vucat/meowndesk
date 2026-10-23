import { createElement, type ComponentType } from 'react';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it } from 'vitest';
import { App } from '../App';
import { MockTransport } from './mock-transport';
import { memoryBadge } from '../telemetry/availability';
import { MemoryAppCard } from '../components/MemoryAppCard';

function mount(scenario = 'full') {
  const transport = new MockTransport(scenario);
  render(createElement(App as ComponentType<{ transport: MockTransport }>, { transport }));
  return transport;
}

async function memoryType() {
  return screen.findByRole('radiogroup', { name: 'Memory type' });
}

it('shows one RAM donut and switches to the independent VRAM denominator', async () => {
  const transport = mount('full');
  const selector = await memoryType();
  expect(within(selector).getByRole('radio', { name: 'System RAM' })).toBeChecked();
  const card = await screen.findByRole('region', { name: 'Memory by Type' });
  const ram = within(card).getByRole('img', { name: /System RAM.*in use/i });
  expect(ram).toHaveAccessibleName(/In Use 11\.2 GB.*Available 20\.8 GB/i);
  expect(within(card).getByText('In Use')).toBeVisible();
  expect(within(card).getByText('Available')).toBeVisible();
  expect(within(card).queryByText('Unclassified')).not.toBeInTheDocument();

  await userEvent.click(within(selector).getByRole('radio', { name: 'Dedicated VRAM' }));
  await waitFor(() => expect(transport.currentView.selection.memoryView).toBe('vram'));
  const vram = await screen.findByRole('img', { name: /Dedicated VRAM.*16\.7% in use/i });
  expect(vram).toHaveAccessibleName(/Used 2 GB.*Free 9 GB.*Unclassified 1 GB/i);
  expect(within(card).getByText(/Partial coverage/)).toBeVisible();
  expect(within(card).getByText('Unclassified')).toBeVisible();
});

it('shows a labeled unavailable VRAM view without fabricating segments', async () => {
  const transport = mount('no_gpu');
  const selector = await memoryType();
  await userEvent.click(within(selector).getByRole('radio', { name: 'Dedicated VRAM' }));
  await waitFor(() => expect(transport.currentView.selection.memoryView).toBe('vram'));
  expect(await screen.findByText(/Dedicated VRAM is unavailable/i)).toBeVisible();
  expect(screen.queryByText('Unclassified')).not.toBeInTheDocument();
});

it('shows ranked Bento tiles, a header total and the accessible shared-page caveat', async () => {
  mount('full');
  const card = await screen.findByRole('region', { name: 'Memory by App' });
  const list = within(card).getByRole('list');
  expect(within(list).getAllByRole('listitem')).toHaveLength(2);
  expect(card.querySelector('.card-heading')).toHaveTextContent('1.7 GB in all');
  expect(within(card).getByTitle(/Shared pages/)).toHaveTextContent('in all');
  expect(within(card).getByText(/Shared pages/, { selector: '.sr-only' })).toBeInTheDocument();
  const blocks = card.querySelectorAll<HTMLElement>('.treemap-block');
  expect(blocks).toHaveLength(2);
  expect(blocks[0]).toHaveTextContent('cödé — 開発環境');
  expect(blocks[0]).toHaveTextContent('1000 MB');
  expect(card.querySelector('.memory-app-bento')).toHaveAttribute('data-columns', '2');
});

it('groups exact app names with BigInt totals while retaining every process alternative', () => {
  const view = structuredClone(new MockTransport('full').currentView);
  const summaries = view.processSummaries!;
  const source = summaries.memoryTop[0]!;
  summaries.memoryTop = ['9007199254740993', '2', '1000000000', '1', '0'].map((bytes, index) => ({
    ...source,
    key: `group-${index}`,
    name: index < 2 ? 'ChatGPT' : `App ${index}`,
    residentBytes: { ...source.residentBytes, value: bytes },
  }));
  const { container } = render(<MemoryAppCard view={view} />);
  const blocks = container.querySelectorAll('.treemap-block');
  expect(blocks).toHaveLength(3);
  expect(blocks[0]).toHaveAttribute('title', 'ChatGPT · 8192 TB · 2 processes');
  expect(within(screen.getByRole('list')).getAllByRole('listitem')).toHaveLength(5);
  expect(screen.getAllByText('ChatGPT', { selector: 'li > span' })).toHaveLength(2);
  expect(summaries.memoryTop[0]?.residentBytes.value).toBe('9007199254740993');
});

it('shows an empty state when no process has readable nonzero memory', () => {
  const view = structuredClone(new MockTransport('full').currentView);
  for (const process of view.processSummaries!.memoryTop) process.residentBytes.value = '0';
  render(<MemoryAppCard view={view} />);
  expect(screen.getByText('No readable memory observations')).toBeVisible();
});

it('ranks by the selected metric and disables unsupported metrics with reasons', async () => {
  const transport = mount('full');
  const card = await screen.findByRole('region', { name: 'Resource Load' });
  const selector = within(card).getByRole('radiogroup', { name: 'Ranking metric' });
  expect(within(selector).getByRole('radio', { name: 'CPU' })).toBeChecked();
  expect(within(card).getByRole('radio', { name: 'GPU' })).toBeEnabled();
  expect(within(card).getByRole('radio', { name: 'Power' })).toBeDisabled();
  expect(within(card).getByText(/per-process power is unavailable/i)).toBeVisible();
  const rows = within(card).getByRole('list').querySelectorAll('li');
  expect(rows).toHaveLength(2);
  expect(rows[0]?.textContent).toContain('%');

  await userEvent.click(within(selector).getByRole('radio', { name: 'GPU' }));
  await waitFor(() => expect(transport.currentView.selection.rankingMetric).toBe('gpu'));
  await waitFor(() => expect(within(card).getAllByRole('listitem')).toHaveLength(1));
  expect(within(card).getByText(/GPU/)).toBeVisible();
});

it('keeps CPU ranking usable when CPU values are unavailable and excludes process control', async () => {
  const transport = mount('no_gpu');
  const card = await screen.findByRole('region', { name: 'Resource Load' });
  const gpu = within(card).getByRole('radio', { name: 'GPU' });
  expect(gpu).toBeDisabled();
  expect(within(card).getByRole('radio', { name: 'CPU' })).toBeChecked();
  expect(
    within(card).queryByRole('button', { name: /kill|terminate|stop process/i }),
  ).not.toBeInTheDocument();
  expect(transport.currentView.selection.rankingMetric).toBe('cpu');
});

it('keeps every breakdown bounded through process churn', async () => {
  const transport = mount('process_churn');
  transport.tick();
  transport.tick();
  const card = await screen.findByRole('region', { name: 'Memory by App' });
  const items = within(card).getAllByRole('listitem');
  expect(items.length).toBeLessThanOrEqual(8);
});

it('uses unrounded RAM percentages for the Normal/Elevated/Critical boundaries', () => {
  expect(memoryBadge(79.99).label).toBe('Normal');
  expect(memoryBadge(80).label).toBe('Elevated');
  expect(memoryBadge(89.99).label).toBe('Elevated');
  expect(memoryBadge(90).label).toBe('Critical');
});

it('keeps compact app names and byte values accessible even with a dominant hero', () => {
  const view = structuredClone(new MockTransport('full').currentView);
  const summaries = view.processSummaries!;
  const source = summaries.memoryTop[0]!;
  const names = [
    'WSL2',
    'VS Code',
    'Chrome',
    'Ollama',
    'Docker',
    'Explorer',
    'OpenCode',
    'tiny-app',
  ];
  summaries.memoryTop = names.map((name, index) => ({
    ...source,
    key: `bento-${index}`,
    name,
    residentBytes: {
      ...source.residentBytes,
      value: index === 0 ? '9000000000' : String(8000 - index * 1000),
    },
  }));
  summaries.displayedMemoryTotalBytes = '9000028000';
  const { container } = render(<MemoryAppCard view={view} />);
  expect(container.querySelectorAll('.treemap-block')).toHaveLength(7);
  expect(container.querySelectorAll('.treemap-compact')).toHaveLength(4);
  const list = screen.getByRole('list');
  expect(within(list).getAllByRole('listitem')).toHaveLength(8);
  names.forEach((name) => expect(within(list).getByText(name)).toBeInTheDocument());
  expect(screen.getByTitle('Ollama · 4.9 KB')).toHaveClass('treemap-compact');
  expect(within(list).getByText('4.9 KB')).toBeInTheDocument();
  expect(
    new Set(
      [...container.querySelectorAll<HTMLElement>('.treemap-badge')].map(
        (badge) => badge.style.background,
      ),
    ).size,
  ).toBeGreaterThanOrEqual(5);
});

it.each([1, 2, 3, 4, 5, 6, 7])('expands the Bento columns for %i readable apps', (count) => {
  const view = structuredClone(new MockTransport('memory_bento').currentView);
  view.processSummaries!.memoryTop = view.processSummaries!.memoryTop.slice(0, count);
  const { container } = render(<MemoryAppCard view={view} />);
  expect(container.querySelector('.memory-app-bento')).toHaveAttribute(
    'data-columns',
    count === 1 ? '1' : count <= 3 ? '2' : '3',
  );
  expect(container.querySelectorAll('.treemap-block')).toHaveLength(count);
  expect(container.querySelectorAll('.treemap-compact')).toHaveLength(Math.max(0, count - 3));
  expect(within(screen.getByRole('list')).getAllByRole('listitem')).toHaveLength(count);
});

it('gives GB tooltips two decimals without rounding bytes before sorting', () => {
  const view = structuredClone(new MockTransport('memory_bento').currentView);
  const summaries = view.processSummaries!;
  const source = summaries.memoryTop[0]!;
  summaries.memoryTop = ['2147483647', '2147483648', '2147483649', '2147483650'].map(
    (bytes, index) => ({
      ...source,
      key: `close-${index}`,
      name: `App ${index}`,
      residentBytes: { ...source.residentBytes, value: bytes },
    }),
  );
  const { container } = render(<MemoryAppCard view={view} />);
  const blocks = container.querySelectorAll('.treemap-block');
  expect([...blocks].map((block) => block.getAttribute('title'))).toEqual([
    'App 3 · 2.00 GB',
    'App 2 · 2.00 GB',
    'App 1 · 2.00 GB',
    'App 0 · 2.00 GB',
  ]);
  expect(blocks[3]).toHaveClass('treemap-compact');
  expect(within(screen.getByRole('list')).getByText('App 0')).toBeInTheDocument();
});
