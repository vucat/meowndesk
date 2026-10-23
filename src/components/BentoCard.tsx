import { useId, type ReactNode } from 'react';
export type Resource = 'cpu' | 'memory' | 'gpu' | 'disk' | 'network' | 'power';
export function BentoCard({
  title,
  resource,
  badge,
  children,
  className = '',
}: {
  title: string;
  resource: Resource;
  badge?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  const id = useId();
  return (
    <section className={`bento-card resource-${resource} ${className}`} aria-labelledby={id}>
      <header className="card-heading">
        <h2 id={id}>
          <span className={`resource-symbol symbol-${resource}`} aria-hidden="true" />
          {title}
        </h2>
        {badge}
      </header>
      {children}
    </section>
  );
}
