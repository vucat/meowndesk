import { useId, type ReactNode } from 'react';
export type Resource = 'cpu' | 'memory' | 'gpu' | 'disk' | 'network' | 'power';
function ResourceIcon({ resource }: { resource: Resource }) {
  return (
    <span className="resource-symbol" aria-hidden="true">
      <svg
        viewBox="0 0 16 16"
        focusable="false"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {resource === 'cpu' && (
          <>
            <rect x="1.75" y="1.75" width="12.5" height="12.5" rx="2.25" />
            <rect x="5.5" y="5.5" width="5" height="5" rx="1" fill="currentColor" stroke="none" />
          </>
        )}
        {resource === 'memory' && (
          <>
            <rect x="2" y="3.25" width="12" height="9.5" rx="1.75" />
            <rect x="2" y="3.25" width="6" height="9.5" fill="currentColor" stroke="none" />
          </>
        )}
        {resource === 'gpu' && (
          <>
            <rect x="2" y="3.25" width="12" height="9.5" rx="1.75" />
            <rect
              x="2.75"
              y="7.75"
              width="10.5"
              height="3.5"
              rx="1"
              fill="currentColor"
              stroke="none"
            />
          </>
        )}
        {resource === 'disk' && (
          <>
            <rect x="2" y="4.25" width="12" height="7.5" rx="1.75" />
            <rect
              x="4.75"
              y="6.75"
              width="6.5"
              height="2.5"
              rx="0.75"
              fill="currentColor"
              stroke="none"
            />
          </>
        )}
        {resource === 'network' && (
          <>
            <path d="M5.25 13V3.5" />
            <path d="m2.75 6 2.5-2.5L7.75 6" />
            <path d="M10.75 3v9.5" />
            <path d="m8.25 10 2.5 2.5 2.5-2.5" />
          </>
        )}
        {resource === 'power' && (
          <path d="M9.75 1.5 3.5 8.75h3.5l-.75 5.75 6.25-7.25H9z" fill="currentColor" />
        )}
      </svg>
    </span>
  );
}
export function BentoCard({
  title,
  resource,
  badge,
  compact = false,
  children,
  className = '',
}: {
  title: string;
  resource: Resource;
  badge?: ReactNode;
  compact?: boolean;
  children: ReactNode;
  className?: string;
}) {
  const id = useId();
  return (
    <section
      className={`bento-card resource-${resource}${compact ? ' bento-card-compact' : ''} ${className}`}
      aria-labelledby={id}
    >
      <header className="card-heading">
        <h2 id={id}>
          <ResourceIcon resource={resource} />
          {title}
        </h2>
        {badge}
      </header>
      {children}
    </section>
  );
}
