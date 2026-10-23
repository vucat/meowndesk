import { createElement, type ComponentType } from 'react';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it } from 'vitest';
import { App } from '../App';
import { MockTransport } from './mock-transport';

function mount(scenario = 'full') {
  const transport = new MockTransport(scenario);
  render(createElement(App as ComponentType<{ transport: MockTransport }>, { transport }));
  return transport;
}

it('exposes 1m/5m/15m/1h as an accessible radio group with 1m selected', async () => {
  mount();
  const group = await screen.findByRole('radiogroup', { name: 'Time range' });
  for (const range of ['1m', '5m', '15m', '1h'])
    expect(within(group).getByRole('radio', { name: range })).toBeVisible();
  expect(within(group).getByRole('radio', { name: '1m' })).toBeChecked();
  expect(screen.getByRole('button', { name: 'Pause' })).toHaveAttribute('aria-pressed', 'false');
});

it('changes every chart to one shared range and keeps focus on the chosen range', async () => {
  const transport = mount();
  await screen.findByRole('radiogroup', { name: 'Time range' });
  await userEvent.click(screen.getByRole('radio', { name: '5m' }));
  await waitFor(() => expect(transport.currentView.selection.range).toBe('5m'));
  expect(screen.getByRole('radio', { name: '5m' })).toBeChecked();
  expect(transport.currentView.history?.range).toBe('5m');
  const figures = screen.getAllByRole('figure');
  expect(figures).toHaveLength(6);
  for (const figure of figures) expect(within(figure).getByText(/^5m · /)).toBeVisible();
});

it('supports keyboard arrow navigation between ranges', async () => {
  const transport = mount();
  await screen.findByRole('radiogroup', { name: 'Time range' });
  const first = screen.getByRole('radio', { name: '1m' });
  first.focus();
  await userEvent.keyboard('{ArrowRight}');
  await waitFor(() => expect(transport.currentView.selection.range).toBe('5m'));
  expect(screen.getByRole('radio', { name: '5m' })).toBeChecked();
  expect(document.activeElement).toBe(screen.getByRole('radio', { name: '5m' }));
});

it('pauses with a visible frozen endpoint and resumes to current data', async () => {
  const transport = mount('full');
  await screen.findByRole('radiogroup', { name: 'Time range' });
  const pause = screen.getByRole('button', { name: 'Pause' });
  await userEvent.click(pause);
  const resume = await screen.findByRole('button', { name: 'Resume' });
  expect(resume).toHaveAttribute('aria-pressed', 'true');
  await waitFor(() => expect(transport.currentView.mode).toBe('paused'));
  const frozen = screen.getByTestId('history-status');
  const frozenText = frozen.textContent;
  expect(frozenText).toMatch(/Frozen at /);
  expect(transport.currentView.inspectionId).not.toBeNull();
  const endpoint = transport.currentView.endpointElapsedMs;

  act(() => {
    transport.tick();
    transport.tick();
  });
  expect(screen.getByTestId('history-status').textContent).toBe(frozenText);
  expect(transport.currentView.endpointElapsedMs).toBeGreaterThan(endpoint);

  await userEvent.click(screen.getByRole('radio', { name: '15m' }));
  await waitFor(() => expect(transport.currentView.selection.range).toBe('15m'));
  expect(screen.getByTestId('history-status').textContent).toBe(frozenText);

  await userEvent.click(screen.getByRole('button', { name: 'Resume' }));
  await waitFor(() => expect(transport.currentView.mode).toBe('live'));
  expect(screen.getByRole('button', { name: 'Pause' })).toHaveAttribute('aria-pressed', 'false');
  expect(screen.getByTestId('history-status')).toHaveTextContent('Live updates');
});

it('reports partial history and gap columns for the selected window', async () => {
  mount();
  await screen.findByRole('radiogroup', { name: 'Time range' });
  const charts = screen.getAllByRole('figure');
  expect(charts).toHaveLength(6);
  for (const chart of charts) {
    expect(within(chart).getByText(/gap columns|60 measured columns/)).toBeVisible();
    expect(within(chart).getByText(/0–[\d.]+ (?:%|W|[KM]B\/s)/)).toBeVisible();
  }
});
