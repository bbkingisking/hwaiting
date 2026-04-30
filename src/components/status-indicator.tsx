import { useEffect, useState } from 'react'
import { getStats } from '@/lib/api'
import { cn } from '@/lib/utils'

export function StatusIndicator() {
  const [dueCount, setDueCount] = useState<number | null>(null)
  const [percentage, setPercentage] = useState<number | null>(null)

  useEffect(() => {
    fetchStats()
    // Refresh stats every 30 seconds
    const interval = setInterval(fetchStats, 30000)
    return () => clearInterval(interval)
  }, [])

  const fetchStats = async () => {
    try {
      const stats = await getStats()
      setDueCount(stats.due_count)
      setPercentage(stats.percentage)
    } catch (err) {
      console.error('Failed to fetch stats:', err)
    }
  }

  const getPercentageColor = () => {
    if (percentage === null) return ''
    if (percentage >= 70) return 'text-green-600 dark:text-green-500'
    if (percentage >= 50) return 'text-yellow-600 dark:text-yellow-500'
    return 'text-destructive'
  }

  return (
    <div className="fixed bottom-4 left-1/2 -translate-x-1/2 flex items-center gap-3 text-sm text-muted-foreground">
      {dueCount !== null && (
        <span>Due: {dueCount}</span>
      )}
      <span className={cn(percentage !== null && getPercentageColor())}>
        {percentage !== null ? `${percentage}%` : '—'}
      </span>
    </div>
  )
}