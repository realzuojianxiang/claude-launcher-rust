export interface SkeletonProps {
  lines?: number;
  label: string;
}

export function Skeleton({ lines = 1, label }: SkeletonProps) {
  return (
    <div className="ui-skeleton" aria-busy="true">
      <span className="ui-skeleton__label" role="status">
        {label}
      </span>
      <div className="ui-skeleton__lines" aria-hidden="true">
        {Array.from({ length: lines }).map((_, i) => (
          <div
            key={i}
            className="ui-skeleton__line"
            data-testid="skeleton-line"
            aria-hidden="true"
          />
        ))}
      </div>
    </div>
  );
}
