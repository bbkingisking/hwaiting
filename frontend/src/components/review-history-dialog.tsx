import { useEffect, useState, useMemo } from 'react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import {
  ChartConfig,
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from '@/components/ui/chart'
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Dot,
} from 'recharts'
import { getReviewHistory, type DayHistory } from '@/lib/api'
import { useSettings } from '@/components/settings-provider'

interface ReviewHistoryDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const chartConfig = {
  percentage: {
    label: 'Correct',
    color: 'white',
  },
} satisfies ChartConfig

function formatDate(dateStr: string): string {
  // dateStr is YYYY-MM-DD in the user's logical day timezone
  const [year, month, day] = dateStr.split('-').map(Number)
  const d = new Date(year, month - 1, day)
  return d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' })
}

function formatShortDate(dateStr: string): string {
  const [year, month, day] = dateStr.split('-').map(Number)
  const d = new Date(year, month - 1, day)
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

export function ReviewHistoryDialog({ open, onOpenChange }: ReviewHistoryDialogProps) {
  const { settings } = useSettings()
  const [days, setDays] = useState<DayHistory[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Derive gradient stop positions from the user's threshold settings.
  // The SVG gradient runs top (y=0 = 100% accuracy) to bottom (y=1 = 0% accuracy),
  // so a threshold percentage p maps to SVG offset (100 - p)%.
  const gradientStops = useMemo(() => {
    const redT = settings?.redThreshold ?? 50
    const yellowT = settings?.yellowThreshold ?? 70
    const greenOffset  = 0
    const yellowOffset = Math.round(100 - yellowT)
    const redOffset    = Math.round(100 - redT)
    const bottomOffset = 100
    return { greenOffset, yellowOffset, redOffset, bottomOffset }
  }, [settings?.redThreshold, settings?.yellowThreshold])

  useEffect(() => {
    if (!open) return
    setIsLoading(true)
    setError(null)
    getReviewHistory()
      .then(res => setDays(res.days))
      .catch(() => setError('Failed to load review history.'))
      .finally(() => setIsLoading(false))
  }, [open])

  const chartData = days.map(d => ({
    date: d.date,
    label: formatShortDate(d.date),
    percentage: Math.round(d.percentage),
    total: d.total,
    correct: d.correct,
  }))

  const hasData = chartData.length > 0

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-130">
        <DialogHeader>
          <DialogTitle>Review History</DialogTitle>
          <DialogDescription>
            Your accuracy over the last 5 days
          </DialogDescription>
        </DialogHeader>

        <div className="pt-2">
          {isLoading && (
            <div className="flex items-center justify-center h-48 text-muted-foreground text-sm">
              Loading…
            </div>
          )}

          {!isLoading && error && (
            <div className="flex items-center justify-center h-48 text-destructive text-sm">
              {error}
            </div>
          )}

          {!isLoading && !error && !hasData && (
            <div className="flex items-center justify-center h-48 text-muted-foreground text-sm">
              No reviews yet. Come back after your first session!
            </div>
          )}

          {!isLoading && !error && hasData && (
            <ChartContainer config={chartConfig} className="h-52 w-full">
              <AreaChart
                data={chartData}
                margin={{ top: 10, right: 16, left: -8, bottom: 0 }}
              >
                <defs>
                  <linearGradient id="fillPercentage" x1="0" y1="0" x2="0" y2="1">
                    <stop offset={`${gradientStops.greenOffset}%`}  stopColor="hsl(142,71%,45%)" stopOpacity={0.35} />
                    <stop offset={`${gradientStops.yellowOffset}%`} stopColor="hsl(45,93%,47%)"  stopOpacity={0.28} />
                    <stop offset={`${gradientStops.redOffset}%`}    stopColor="hsl(0,84%,60%)"   stopOpacity={0.35} />
                    <stop offset={`${gradientStops.bottomOffset}%`} stopColor="hsl(0,84%,60%)"   stopOpacity={0.35} />
                  </linearGradient>
                </defs>
                <CartesianGrid vertical={false} strokeDasharray="3 3" className="stroke-border" />
                <XAxis
                  dataKey="label"
                  tickLine={false}
                  axisLine={false}
                  tick={{ fontSize: 12 }}
                  className="fill-muted-foreground"
                />
                <YAxis
                  domain={[0, 100]}
                  tickFormatter={v => `${v}%`}
                  tickLine={false}
                  axisLine={false}
                  tick={{ fontSize: 12 }}
                  className="fill-muted-foreground"
                  width={52}
                />
                <ChartTooltip
                  content={
                    <ChartTooltipContent
                      formatter={(value, _name, item) => (
                        <div className="flex flex-col gap-0.5">
                          <span className="text-muted-foreground text-xs">
                            {formatDate(item.payload.date)}
                          </span>
                          <span className="font-semibold">
                            {value}% correct
                          </span>
                          <span className="text-muted-foreground text-xs">
                            {item.payload.correct} / {item.payload.total} cards
                          </span>
                        </div>
                      )}
                      hideLabel
                    />
                  }
                />
                <Area
                  type="monotone"
                  dataKey="percentage"
                  stroke="white"
                  strokeWidth={2}
                  fill="url(#fillPercentage)"
                  connectNulls={false}
                  dot={<Dot r={4} fill="white" strokeWidth={0} />}
                  activeDot={{ r: 5, fill: "white", strokeWidth: 0 }}
                />
              </AreaChart>
            </ChartContainer>
          )}
        </div>

        {!isLoading && !error && hasData && (
          <div className="flex justify-between px-1 pt-1">
            {days.map(d => (
              <div key={d.date} className="flex flex-col items-center gap-0.5">
                <span className="text-xs font-semibold">{Math.round(d.percentage)}%</span>
                <span className="text-xs text-muted-foreground">{formatShortDate(d.date)}</span>
              </div>
            ))}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}