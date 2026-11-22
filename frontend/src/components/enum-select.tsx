import type { DisplayOption } from '@/lib/vocabulary'

export function EnumSelect({ id, options, value, onChange, allowEmpty = true }: {
  allowEmpty?: boolean
  id?: string
  options: Record<string, DisplayOption>
  value: string
  onChange: (e: React.ChangeEvent<HTMLSelectElement>) => void
}) {
  return (
    <select
      id={id}
      value={value}
      onChange={onChange}
      className='w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring'
    >
      {allowEmpty && <option value=''>—</option>}
      {Object.values(options).map((o) => (
        <option key={o.slug} value={o.slug}>
          {o.endings && o.endings.length > 0 ? `${o.label} (${o.endings.join(', ')})` : o.label}
        </option>
      ))}
    </select>
  )
}
