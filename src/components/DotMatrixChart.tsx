import { useEffect, useId, useRef } from 'react';
import type { Chart } from '../telemetry/types';
import { formatNumber, formatRate } from '../telemetry/format';
import type { Resource } from './BentoCard';
const colors: Record<Resource, string> = {
  cpu: '#3b93ff',
  memory: '#32d66b',
  gpu: '#f04089',
  disk: '#f5a623',
  network: '#39c6bd',
  power: '#f5cf48',
};
function drawDots(
  ctx: CanvasRenderingContext2D,
  chart: Chart | null,
  width: number,
  height: number,
  color: string,
): void {
  ctx.clearRect(0, 0, width, height);
  const stepX = width / 60,
    stepY = height / 8;
  const radius = Math.max(0.5, Math.min(1.9, stepX * 0.28));
  for (let column = 0; column < 60; column++) {
    const point = chart?.columns[column];
    const value = point?.value ?? null;
    const lit =
      value === null || value === 0
        ? 0
        : Math.min(8, Math.max(1, Math.ceil((value / (chart?.scaleMax ?? 1)) * 8)));
    for (let row = 0; row < 8; row++) {
      const x = (column + 0.5) * stepX,
        y = height - (row + 0.5) * stepY;
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.fillStyle = row < lit ? color : '#2b2b31';
      ctx.globalAlpha = point?.hasGap && row < lit ? 0.55 : 1;
      ctx.fill();
      if (value === 0 && row === 0) {
        ctx.strokeStyle = color;
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    }
  }
  ctx.globalAlpha = 1;
}
export function DotMatrixChart({
  chart,
  resource,
  label,
  range = '1m',
}: {
  chart: Chart | null;
  resource: Resource;
  label: string;
  range?: string;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const captionId = useId();
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    let animation = 0;
    const draw = () => {
      const { width, height } = element.getBoundingClientRect();
      if (width <= 0 || height <= 0) return;
      const dpr = window.devicePixelRatio || 1;
      element.width = Math.round(width * dpr);
      element.height = Math.round(height * dpr);
      const ctx = element.getContext('2d');
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      drawDots(ctx, chart, width, height, colors[resource]);
    };
    const schedule = () => {
      cancelAnimationFrame(animation);
      animation = requestAnimationFrame(draw);
    };
    const observer = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(schedule) : null;
    observer?.observe(element);
    window.addEventListener('resize', schedule);
    schedule();
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', schedule);
      cancelAnimationFrame(animation);
    };
  }, [chart, resource]);
  const gapCount = chart?.columns.filter((p) => p.hasGap).length ?? 60;
  const zeros = chart?.columns.filter((p) => p.value === 0).length ?? 0;
  const unit =
    chart?.unit ??
    (['cpu', 'memory', 'gpu'].includes(resource)
      ? 'pct'
      : resource === 'power'
        ? 'watts'
        : 'bytes_per_second');
  const units = unit === 'pct' ? '%' : unit === 'watts' ? 'W' : 'B/s';
  const scale = chart?.scaleMax ?? (units === '%' ? 100 : 1);
  const scaleText =
    unit === 'bytes_per_second' ? formatRate(scale) : `${formatNumber(scale)} ${units}`;
  return (
    <figure className="dot-chart" aria-labelledby={captionId}>
      <canvas ref={canvas} aria-hidden="true" />
      <figcaption id={captionId}>
        <span>
          {range} · {label}
        </span>
        <span>0–{scaleText}</span>
        <span className="sr-only">
          {chart?.scopeLabel ?? 'Scope unavailable'} ·{' '}
          {chart?.partialHistory || gapCount
            ? `Partial history · ${gapCount} gap columns`
            : '60 measured columns'}
          {zeros > 0 ? ` · ${zeros} measured zero columns` : ''}
        </span>
      </figcaption>
    </figure>
  );
}
