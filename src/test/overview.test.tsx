import { createElement, type ComponentType } from 'react';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it } from 'vitest';
import { App } from '../App';
import { MockTransport } from './mock-transport';
import { expectedSummaries } from './overview-expectations';
const cards = [
  'CPU',
  'Memory',
  'GPU & VRAM',
  'Disk',
  'Network',
  'Power/Thermals',
  'Memory by Type',
  'Memory by App',
  'Resource Load',
];
function mount(scenario = 'full') {
  const transport = new MockTransport(scenario);
  render(createElement(App as ComponentType<{ transport: MockTransport }>, { transport }));
  return transport;
}
it('keeps all nine named card shells and six matching sidebar summaries visible', async () => {
  const transport = mount();
  for (const name of cards) expect(await screen.findByRole('region', { name })).toBeVisible();
  const sidebar = screen.getByRole('complementary');
  for (const name of ['CPU', 'Memory', 'GPU', 'Disk', 'Network', 'Power'])
    expect(within(sidebar).getByText(name, { exact: true })).toBeVisible();
  for (const [resource, expected] of Object.entries(expectedSummaries(transport.currentView))) {
    expect(screen.getByTestId(`${resource}-summary`)).toHaveTextContent(expected);
  }
});
it.each(['no_gpu', 'denied', 'stale', 'no_active_interface'])(
  'preserves the grid with %s availability',
  async (scenario) => {
    mount(scenario);
    for (const name of cards) expect(await screen.findByRole('region', { name })).toBeVisible();
    expect(
      screen.queryByRole('button', { name: /kill|terminate|stop process/i }),
    ).not.toBeInTheDocument();
    if (scenario === 'denied') expect(screen.getByText(/Degraded monitoring/i)).toBeVisible();
    if (scenario === 'no_active_interface')
      expect(screen.getByText(/No active interfaces/i)).toBeVisible();
  },
);
it('labels guest scope, permits keyboard focus, and has no process navigation', async () => {
  mount('wsl2');
  expect(await screen.findByText(/WSL2.*guest|guest.*WSL2/i)).toBeVisible();
  await userEvent.tab();
  expect(document.activeElement).not.toBe(document.body);
  expect(screen.queryByRole('link', { name: /^Processes$/ })).not.toBeInTheDocument();
});

it('shows initializing shells before a first sample and a visible bootstrap failure', async () => {
  const transport = new MockTransport('initializing');
  const { unmount } = render(
    createElement(App as ComponentType<{ transport: MockTransport }>, { transport }),
  );
  expect(await screen.findByText(/Initializing/i)).toBeVisible();
  for (const name of cards) expect(screen.getByRole('region', { name })).toBeVisible();
  unmount();
  transport.bootstrap = () => ({
    ok: false,
    error: { code: 'internal', message: 'Monitoring could not start', retryable: true },
  });
  render(createElement(App as ComponentType<{ transport: MockTransport }>, { transport }));
  expect(await screen.findByText('Monitoring could not start')).toBeVisible();
});
