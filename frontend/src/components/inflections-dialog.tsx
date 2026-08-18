import type { CardInflection } from '@/lib/api'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useFieldValues } from '@/components/field-values-provider'

interface InflectionsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  word: string
  pos: string
  inflections: CardInflection[]
}

// Groups this card's resolved inflections by their catalog category
// (present/past/future/...), in the catalog's own sort_order - both the
// category order and the row order within it come from
// inflectionForm/inflection_categories via useFieldValues, not from the
// order `inflections` itself arrived in.
function groupByCategory(
  inflections: CardInflection[],
  inflectionForm: ReturnType<typeof useFieldValues>['inflectionForm'],
  pos: string,
) {
  const byCategory = new Map<string, { label: string; rows: { label: string; ending: string; form: string; sortOrder: number }[] }>()

  for (const { form_slug, form } of inflections) {
    const meta = inflectionForm[form_slug]
    if (!meta) continue // catalog hasn't loaded yet, or slug is stale
    if (meta.verb_only && pos !== '동사') continue

    if (!byCategory.has(meta.category_slug)) {
      byCategory.set(meta.category_slug, { label: meta.category_label_en, rows: [] })
    }
    byCategory.get(meta.category_slug)!.rows.push({
      label: meta.label_en,
      ending: meta.ending_ko,
      form,
      sortOrder: meta.sort_order,
    })
  }

  for (const category of byCategory.values()) {
    category.rows.sort((a, b) => a.sortOrder - b.sortOrder)
  }

  // Category insertion order already follows sort_order, since `inflections`
  // itself arrives ordered by it (see check_answer's query) - Map preserves
  // insertion order, so no extra sort is needed here.
  return [...byCategory.values()]
}

export function InflectionsDialog({ open, onOpenChange, word, pos, inflections }: InflectionsDialogProps) {
  const { inflectionForm } = useFieldValues()
  const categories = groupByCategory(inflections, inflectionForm, pos)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-125 max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{word} — conjugation</DialogTitle>
          <DialogDescription>All resolved inflected forms for this word</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {categories.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-8">
              No conjugation table available for this word yet
            </div>
          ) : (
            categories.map((category) => (
              <div key={category.label}>
                <h3 className="text-sm font-medium text-muted-foreground mb-1.5">{category.label}</h3>
                <table className="w-full text-sm">
                  <tbody>
                    {category.rows.map((row) => (
                      <tr key={row.label} className="border-t border-border/50 first:border-t-0">
                        <td className="py-1 pr-3 text-muted-foreground whitespace-nowrap">{row.label}</td>
                        <td className="py-1 font-medium">{row.form}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ))
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
