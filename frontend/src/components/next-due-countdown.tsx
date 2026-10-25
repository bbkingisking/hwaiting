import { useTimeUntil } from '@/hooks/use-time-until'

export function NextDueCountdown({ dueAt }: { dueAt: string | null }) {
  const remaining = useTimeUntil(dueAt)
  return remaining ? <p className="text-sm">Next in {remaining}</p> : null
}
