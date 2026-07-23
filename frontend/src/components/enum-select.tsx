import type { EnumEntry } from '@/lib/api'

export function EnumSelect({ id, options, value, onChange }: {
  id?: string
  options: Record<string, EnumEntry>
  value: string
  onChange: (e: React.ChangeEvent<HTMLSelectElement>) => void
}) {
  return (
    <select
      id={id}
      value={value}
      onChange={onChange}
      className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <option value="">—</option>
      {Object.values(options).map(o => (
        <option key={o.slug} value={o.slug}>{o.endings ? `${o.label} (${o.endings})` : o.label}</option>
      ))}
    </select>
  )
}
