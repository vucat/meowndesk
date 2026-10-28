// Test-only entry; production main.tsx never imports fixtures.
import { createElement, type ComponentType } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from '../../../src/App';
import { MockTransport } from '../../../src/test/mock-transport';
import '../../../src/styles.css';
const params = new URLSearchParams(location.search);
const transport = new MockTransport(params.get('scenario') ?? 'full');
createRoot(document.getElementById('root')!).render(
  createElement(App as ComponentType<{ transport: MockTransport }>, { transport }),
);
if (params.has('advance') && !params.has('silence')) {
  const startTicks = () => setInterval(() => transport.tick(), 1000);
  const resumeAfterMs = Number(params.get('resumeAfterMs') ?? 0);
  if (resumeAfterMs > 0) setTimeout(startTicks, resumeAfterMs);
  else startTicks();
}
