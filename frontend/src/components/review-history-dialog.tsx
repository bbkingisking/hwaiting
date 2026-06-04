import { useEffect, useState, useMemo } from 'react'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'
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
  ReferenceLine,
  Dot,
} from 'recharts'
import { getReviewHistory, getHistorySummary, getHistoryBreakdown, type DayHistory, type HistorySummary, type BreakdownRow } from '@/lib/api'
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

function dotColor(percentage: number, redThreshold: number, yellowThreshold: number): string {
  if (percentage >= yellowThreshold) return 'hsla(142,71%,45%,0.55)'
  if (percentage >= redThreshold)    return 'hsla(45,93%,47%,0.55)'
  return 'hsla(0,84%,60%,0.55)'
}

function formatFirstDate(dateStr: string): string {
  const [year, month, day] = dateStr.split('-').map(Number)
  const d = new Date(year, month - 1, day)
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

function SummaryStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between items-baseline">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="text-sm font-semibold tabular-nums">{value}</span>
    </div>
  )
}

function accuracyBarColor(percentage: number, redThreshold: number, yellowThreshold: number): string {
  if (percentage >= yellowThreshold) return 'bg-green-500'
  if (percentage >= redThreshold)    return 'bg-yellow-500'
  return 'bg-red-500'
}

function BreakdownTable({ title, rows, redThreshold, yellowThreshold }: {
  title: string
  rows: BreakdownRow[]
  redThreshold: number
  yellowThreshold: number
}) {
  if (rows.length === 0) return null

  const maxReviews = Math.max(...rows.map(r => r.reviews))

  return (
    <div>
      <h4 className="text-xs font-semibold text-muted-foreground mb-2 uppercase tracking-wide">
        {title}
      </h4>
      <div className="flex flex-col gap-1.5">
        {rows.map(row => (
          <div key={row.label} className="flex items-center gap-2">
            <span className="text-xs w-24 shrink-0 truncate" title={row.label}>
              {row.label}
            </span>
            <div className="flex-1 h-3 bg-muted rounded-sm overflow-hidden">
              <div
                className={`h-full rounded-sm ${accuracyBarColor(row.accuracy, redThreshold, yellowThreshold)}`}
                style={{ width: `${Math.max((row.reviews / maxReviews) * 100, 2)}%` }}
              />
            </div>
            <span className="text-xs font-semibold tabular-nums w-12 text-right">
              {Math.round(row.accuracy)}%
            </span>
            <span className="text-xs text-muted-foreground tabular-nums w-10 text-right">
              ({row.reviews})
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

export function ReviewHistoryDialog({ open, onOpenChange }: ReviewHistoryDialogProps) {
  const { settings, updateSettings } = useSettings()
  const [days, setDays] = useState<DayHistory[]>([])
  const [summary, setSummary] = useState<HistorySummary | null>(null)
  const [breakdownPos, setBreakdownPos] = useState<BreakdownRow[]>([])
  const [breakdownOrigin, setBreakdownOrigin] = useState<BreakdownRow[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const colorizedArea     = settings?.historyColorizedArea  ?? false
  const colorizedDots     = settings?.historyColoredDots    ?? false
  const showThresholdLines = settings?.historyThresholdLines ?? false

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
    Promise.all([getReviewHistory(), getHistorySummary(), getHistoryBreakdown()])
      .then(([historyRes, summaryRes, breakdownRes]) => {
        setDays(historyRes.days)
        setSummary(summaryRes)
        setBreakdownPos(breakdownRes.by_pos)
        setBreakdownOrigin(breakdownRes.by_origin)
      })
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
            Your accuracy over the last 5 days and all-time summary
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
            <ChartContainer config={chartConfig} className="aspect-auto h-40 sm:h-52 w-full">
              <AreaChart
                data={chartData}
                margin={{ top: 10, right: 8, left: -8, bottom: 0 }}
              >
                <defs>
                  <linearGradient id="fillPercentage" x1="0" y1="0" x2="0" y2="1">
                    {colorizedArea ? (
                      <>
                        <stop offset={`${gradientStops.greenOffset}%`}  stopColor="hsl(142,71%,45%)" stopOpacity={0.35} />
                        <stop offset={`${gradientStops.yellowOffset}%`} stopColor="hsl(45,93%,47%)"  stopOpacity={0.28} />
                        <stop offset={`${gradientStops.redOffset}%`}    stopColor="hsl(0,84%,60%)"   stopOpacity={0.35} />
                        <stop offset={`${gradientStops.bottomOffset}%`} stopColor="hsl(0,84%,60%)"   stopOpacity={0.35} />
                      </>
                    ) : (
                      <>
                        <stop offset="5%"  stopColor="var(--chart-1)" stopOpacity={0.3} />
                        <stop offset="95%" stopColor="var(--chart-1)" stopOpacity={0.02} />
                      </>
                    )}
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
                  tick={{ fontSize: 11 }}
                  className="fill-muted-foreground"
                  width={40}
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
                {showThresholdLines && (
                  <>
                    <ReferenceLine
                      y={gradientStops.yellowOffset === 0 ? undefined : settings?.yellowThreshold ?? 70}
                      stroke="hsl(45,93%,47%)"
                      strokeWidth={1}
                      strokeDasharray="4 4"
                      strokeOpacity={0.5}
                    />
                    <ReferenceLine
                      y={settings?.redThreshold ?? 50}
                      stroke="hsl(0,84%,60%)"
                      strokeWidth={1}
                      strokeDasharray="4 4"
                      strokeOpacity={0.5}
                    />
                  </>
                )}
                <Area
                  type="monotone"
                  dataKey="percentage"
                  stroke="white"
                  strokeWidth={2}
                  fill="url(#fillPercentage)"
                  connectNulls={false}
                  dot={(props: any) => {
                    const { cx, cy, payload } = props
                    const color = colorizedDots
                      ? dotColor(payload.percentage, settings?.redThreshold ?? 50, settings?.yellowThreshold ?? 70)
                      : 'white'
                    return <Dot key={payload.date} cx={cx} cy={cy} r={4} fill={color} strokeWidth={0} />
                  }}
                  activeDot={(props: any) => {
                    const { cx, cy, payload } = props
                    const color = colorizedDots
                      ? dotColor(payload.percentage, settings?.redThreshold ?? 50, settings?.yellowThreshold ?? 70)
                      : 'white'
                    return <Dot key={payload.date} cx={cx} cy={cy} r={5} fill={color} strokeWidth={0} />
                  }}
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

        {!isLoading && !error && summary && summary.total_reviews > 0 && (
          <div className="border-t pt-3 mt-1">
            <h4 className="text-xs font-semibold text-muted-foreground mb-2.5 uppercase tracking-wide">
              History Summary
            </h4>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-x-4 gap-y-2">
              <SummaryStat label="Total Reviews" value={summary.total_reviews.toLocaleString()} />
              <SummaryStat label="Cards Reviewed" value={summary.total_cards_reviewed.toLocaleString()} />
              <SummaryStat label="Accuracy" value={`${summary.total_accuracy.toFixed(1)}%`} />
              <SummaryStat label="Avg / Day" value={summary.avg_reviews_per_day.toFixed(1)} />
              <SummaryStat label="Current Streak" value={`${summary.current_streak} day${summary.current_streak !== 1 ? 's' : ''}`} />
              <SummaryStat label="Longest Streak" value={`${summary.longest_streak} day${summary.longest_streak !== 1 ? 's' : ''}`} />
              <SummaryStat label="Learning" value={summary.cards_learning.toLocaleString()} />
              <SummaryStat label="Mastered" value={summary.cards_review.toLocaleString()} />
              {summary.cards_relearning > 0 && (
                <SummaryStat label="Relearning" value={summary.cards_relearning.toLocaleString()} />
              )}
              {summary.first_review_date && (
                <SummaryStat label="Studying Since" value={formatFirstDate(summary.first_review_date)} />
              )}
            </div>
          </div>
        )}

        {!isLoading && !error && (breakdownPos.length > 0 || breakdownOrigin.length > 0) && (
          <div className="border-t pt-3 mt-1 flex flex-col gap-3">
            <BreakdownTable
              title="By Part of Speech"
              rows={breakdownPos}
              redThreshold={settings?.redThreshold ?? 80}
              yellowThreshold={settings?.yellowThreshold ?? 90}
            />
            <BreakdownTable
              title="By Origin"
              rows={breakdownOrigin}
              redThreshold={settings?.redThreshold ?? 80}
              yellowThreshold={settings?.yellowThreshold ?? 90}
            />
          </div>
        )}

        <div className="flex flex-col gap-2.5 border-t pt-4 mt-1">
          <div className="flex items-center justify-between">
            <Label htmlFor="toggle-area" className="text-sm text-muted-foreground cursor-pointer">Colorized area fill</Label>
            <Switch id="toggle-area" checked={colorizedArea} onCheckedChange={v => updateSettings({ historyColorizedArea: v })} />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="toggle-dots" className="text-sm text-muted-foreground cursor-pointer">Accuracy-colored dots</Label>
            <Switch id="toggle-dots" checked={colorizedDots} onCheckedChange={v => updateSettings({ historyColoredDots: v })} />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="toggle-lines" className="text-sm text-muted-foreground cursor-pointer">Threshold lines</Label>
            <Switch id="toggle-lines" checked={showThresholdLines} onCheckedChange={v => updateSettings({ historyThresholdLines: v })} />
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
