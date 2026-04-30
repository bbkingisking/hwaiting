import { Settings } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { useSettings } from '@/components/settings-provider'
import { THRESHOLD_CONSTRAINTS } from '@/lib/constants'

export function SettingsDialog() {
  const { settings, updateSettings } = useSettings()

  return (
    <Dialog>
      <DialogTrigger render={<Button variant="outline" size="icon" />}>
        <Settings className="h-[1.2rem] w-[1.2rem]" />
        <span className="sr-only">Settings</span>
      </DialogTrigger>
      <DialogContent className="sm:max-w-106.25">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>
            Customize your flashcard experience
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-6 py-4">
          {/* Show Percentage Toggle */}
          <div className="flex items-center justify-between">
            <Label htmlFor="show-percentage" className="flex-1">
              Show correct rate percentage
            </Label>
            <Switch
              id="show-percentage"
              checked={settings.showPercentage}
              onCheckedChange={(checked) => updateSettings({ showPercentage: checked })}
            />
          </div>

          {/* Color Thresholds */}
          {settings.showPercentage && (
            <>
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <Label htmlFor="red-threshold">Red threshold (below)</Label>
                  <span className="text-sm text-muted-foreground">{settings.redThreshold}%</span>
                </div>
                <Slider
                  id="red-threshold"
                  min={THRESHOLD_CONSTRAINTS.MIN}
                  max={THRESHOLD_CONSTRAINTS.MAX}
                  step={THRESHOLD_CONSTRAINTS.STEP}
                  value={settings.redThreshold}
                  onValueChange={(value) => updateSettings({ redThreshold: value as number })}
                  className="**:[[role=slider]]:bg-destructive"
                />
                <p className="text-xs text-muted-foreground">
                  Cards below this percentage will be red
                </p>
              </div>

              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <Label htmlFor="yellow-threshold">Yellow threshold (below)</Label>
                  <span className="text-sm text-muted-foreground">{settings.yellowThreshold}%</span>
                </div>
                <Slider
                  id="yellow-threshold"
                  min={THRESHOLD_CONSTRAINTS.MIN}
                  max={THRESHOLD_CONSTRAINTS.MAX}
                  step={THRESHOLD_CONSTRAINTS.STEP}
                  value={settings.yellowThreshold}
                  onValueChange={(value) => updateSettings({ yellowThreshold: value as number })}
                  className="**:[[role=slider]]:bg-yellow-600"
                />
                <p className="text-xs text-muted-foreground">
                  Cards below this percentage will be yellow
                </p>
              </div>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}