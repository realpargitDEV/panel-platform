/**
 * The heading block every page opens with: a breadcrumb, a small uppercase
 * accent label, a large title, then a muted line of explanation.
 */
export default function PageHeader({
  breadcrumb,
  label,
  title,
  subtitle,
}: {
  breadcrumb: string;
  label: string;
  title: string;
  subtitle: string;
}) {
  return (
    <div className="mb-6">
      <p className="text-xs text-neutral-500">{breadcrumb}</p>
      <p className="mt-3 text-xs font-semibold uppercase tracking-wider text-accent">{label}</p>
      <h1 className="mt-1 text-3xl font-bold tracking-tight">{title}</h1>
      <p className="mt-1.5 text-sm text-neutral-400">{subtitle}</p>
    </div>
  );
}
