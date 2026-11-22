import type { CardInflection } from '@/lib/api'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { groupConjugations } from '@/lib/conjugation-presentation'

interface InflectionsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  word: string
  inflections: CardInflection[]
}

export function InflectionsDialog({ open, onOpenChange, word, inflections }: InflectionsDialogProps) {
  const categories = groupConjugations(inflections)

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
                      <tr key={row.form_slug} className="border-t border-border/50 first:border-t-0">
                        <td title={row.ending} className="py-1 pr-3 text-muted-foreground whitespace-nowrap">{row.label}</td>
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
