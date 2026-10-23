import { createElement, type ComponentType } from 'react';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it } from 'vitest';
import { App } from '../App';
import { MockTransport } from './mock-transport';

function mount() {
  const transport = new MockTransport('full');
  render(createElement(App as ComponentType<{ transport: MockTransport }>, { transport }));
  return transport;
}

async function openSettings() {
  const trigger = await screen.findByRole('button', { name: 'Settings' });
  await userEvent.click(trigger);
  return screen.findByRole('dialog', { name: 'Settings' });
}

it('opens an accessible Settings dialog with only interval and GPU choices', async () => {
  mount();
  const dialog = await openSettings();
  expect(within(dialog).getByRole('radio', { name: '1 second' })).toBeChecked();
  expect(within(dialog).getByRole('radio', { name: '2 seconds' })).toBeVisible();
  expect(within(dialog).getByRole('radio', { name: '5 seconds' })).toBeVisible();
  expect(within(dialog).getByRole('radio', { name: /NVIDIA GeForce RTX 3060/ })).toBeChecked();
  expect(within(dialog).queryByRole('radio', { name: /AMD Radeon 780M/ })).not.toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: 'Save' })).toBeVisible();
  expect(within(dialog).getByRole('button', { name: 'Cancel' })).toBeVisible();
  expect(document.activeElement).toBe(within(dialog).getByRole('radio', { name: '1 second' }));
});

it('Save applies the interval, closes and restores focus to the trigger', async () => {
  const transport = mount();
  const dialog = await openSettings();
  await userEvent.click(within(dialog).getByRole('radio', { name: '5 seconds' }));
  await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(transport.currentView.preferences.intervalMs).toBe(5000);
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Settings' }));

  const reopened = await openSettings();
  expect(within(reopened).getByRole('radio', { name: '5 seconds' })).toBeChecked();
  await userEvent.click(within(reopened).getByRole('button', { name: 'Cancel' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(transport.currentView.preferences.intervalMs).toBe(5000);
});

it('Cancel preserves the previous interval', async () => {
  const transport = mount();
  const dialog = await openSettings();
  await userEvent.click(within(dialog).getByRole('radio', { name: '2 seconds' }));
  await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(transport.currentView.preferences.intervalMs).toBe(1000);
});

it('allows interval changes while paused but disables GPU selection with Resume-first text', async () => {
  const transport = mount();
  await userEvent.click(await screen.findByRole('button', { name: 'Pause' }));
  await waitFor(() => expect(transport.currentView.mode).toBe('paused'));
  const endpoint = transport.currentView.endpointElapsedMs;
  const dialog = await openSettings();
  expect(within(dialog).getByRole('radio', { name: /NVIDIA GeForce RTX 3060/ })).toBeDisabled();
  expect(within(dialog).getByText(/Resume monitoring to change/i)).toBeVisible();
  await userEvent.click(within(dialog).getByRole('radio', { name: '2 seconds' }));
  await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(transport.currentView.preferences.intervalMs).toBe(2000);
  expect(transport.currentView.endpointElapsedMs).toBe(endpoint);
  expect(transport.currentView.sample?.intervalMs).toBe(1000);
});

it('reports a persistence failure without undoing the in-session choice', async () => {
  const transport = mount();
  transport.setPreferences = () => {
    const view = structuredClone(transport.currentView);
    view.preferences.intervalMs = 5000;
    view.revision += 1;
    view.notices.push({
      code: 'preferences_not_saved',
      message: 'Preferences could not be saved; in-session choices are still active',
    });
    return { ok: true, data: { view, persisted: false } };
  };
  const dialog = await openSettings();
  await userEvent.click(within(dialog).getByRole('radio', { name: '5 seconds' }));
  await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(screen.getByText(/Preferences could not be saved/i)).toBeVisible();
  const reopened = await openSettings();
  expect(within(reopened).getByRole('radio', { name: '5 seconds' })).toBeChecked();
});

it('preserves modal focus through advancing live samples and still traps and restores focus', async () => {
  const transport = mount();
  const dialog = await openSettings();
  const save = within(dialog).getByRole('button', { name: 'Save' });
  await userEvent.click(within(dialog).getByRole('radio', { name: '5 seconds' }));
  save.focus();
  act(() => transport.tick());
  expect(save).toHaveFocus();
  await userEvent.tab();
  expect(within(dialog).getByRole('radio', { name: '5 seconds' })).toHaveFocus();
  await userEvent.keyboard('{Escape}');
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Settings' })).toHaveFocus();
});
