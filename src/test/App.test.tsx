import { render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';
import { App } from '../App';
it('boots the accessible Overview and keeps unavailable browser telemetry explicit', async () => {
  render(<App />);
  expect(screen.getByRole('main')).toBeInTheDocument();
  expect(screen.getByRole('heading', { name: 'Overview', level: 1 })).toBeVisible();
  expect(
    await screen.findByText('Hardware telemetry is unavailable in this browser'),
  ).toBeVisible();
  expect(screen.getAllByRole('region')).toHaveLength(9);
  expect(
    screen.queryByRole('button', { name: /kill|terminate|stop process/i }),
  ).not.toBeInTheDocument();
});
