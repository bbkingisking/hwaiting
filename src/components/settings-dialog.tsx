import { Trash2, Copy } from 'lucide-react'
import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { Separator } from '@/components/ui/separator'
import { useSettings } from '@/components/settings-provider'
import { useAuth } from '@/components/auth-provider'
import { THRESHOLD_CONSTRAINTS } from '@/lib/constants'

interface InviteCode {
  code: string
  created_at: string
  used_at: string | null
  used_by_username: string | null
}

interface SettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SettingsDialog({ open, onOpenChange }: SettingsDialogProps) {
  const { settings, updateSettings } = useSettings()
  const { token, isAdmin } = useAuth()
  const [inviteCodes, setInviteCodes] = useState<InviteCode[]>([])
  const [isLoadingInvites, setIsLoadingInvites] = useState(false)
  const [isGenerating, setIsGenerating] = useState(false)

  const fetchInviteCodes = async () => {
    if (!isAdmin || !token) return

    setIsLoadingInvites(true)
    try {
      const response = await fetch(`${window.location.origin}/api/admin/invites`, {
        headers: {
          'Authorization': `Bearer ${token}`,
        },
      })
      if (response.ok) {
        const data = await response.json()
        setInviteCodes(data.codes)
      }
    } catch (error) {
      console.error('Failed to fetch invite codes:', error)
    } finally {
      setIsLoadingInvites(false)
    }
  }

  const generateInvites = async () => {
    if (!isAdmin || !token) return

    setIsGenerating(true)
    try {
      const response = await fetch(`${window.location.origin}/api/admin/invites`, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ count: 1 }),
      })
      if (response.ok) {
        await fetchInviteCodes()
      }
    } catch (error) {
      console.error('Failed to generate invite codes:', error)
    } finally {
      setIsGenerating(false)
    }
  }

  const deleteInvite = async (code: string) => {
    if (!isAdmin || !token) return

    try {
      const response = await fetch(`${window.location.origin}/api/admin/invites/${code}`, {
        method: 'DELETE',
        headers: {
          'Authorization': `Bearer ${token}`,
        },
      })
      if (response.ok) {
        await fetchInviteCodes()
      }
    } catch (error) {
      console.error('Failed to delete invite code:', error)
    }
  }

  const copyToClipboard = async (code: string) => {
    try {
      await navigator.clipboard.writeText(code)
    } catch (error) {
      console.error('Failed to copy to clipboard:', error)
    }
  }

  useEffect(() => {
    if (isAdmin) {
      fetchInviteCodes()
    }
  }, [isAdmin])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
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

          {/* Admin Section */}
          {isAdmin && (
            <>
              <Separator />
              <div className="space-y-4">
                <div>
                  <h3 className="text-lg font-semibold">Admin Stuff</h3>
                  <p className="text-sm text-muted-foreground">
                    Manage invite codes
                  </p>
                </div>

                <div className="flex gap-2">
                  <Button
                    onClick={generateInvites}
                    disabled={isGenerating}
                    variant="outline"
                  >
                    {isGenerating ? 'Generating...' : 'Generate Invite'}
                  </Button>
                  <Button
                    onClick={fetchInviteCodes}
                    disabled={isLoadingInvites}
                    variant="outline"
                  >
                    Refresh
                  </Button>
                </div>

                <div className="space-y-2 max-h-64 overflow-y-auto">
                  {isLoadingInvites ? (
                    <p className="text-sm text-muted-foreground">Loading...</p>
                  ) : inviteCodes.length === 0 ? (
                    <p className="text-sm text-muted-foreground">No invite codes yet</p>
                  ) : (
                    inviteCodes.map((invite) => (
                      <div
                        key={invite.code}
                        className="flex items-center justify-between p-2 border rounded-md"
                      >
                        <div className="flex-1">
                          <code className="font-mono text-sm">{invite.code}</code>
                          {invite.used_at ? (
                            <span className="ml-2 text-xs text-muted-foreground">
                              (used by {invite.used_by_username})
                            </span>
                          ) : (
                            <span className="ml-2 text-xs text-green-600">
                              available
                            </span>
                          )}
                        </div>
                        <div className="flex gap-1">
                          <Button
                            size="icon"
                            variant="ghost"
                            onClick={() => copyToClipboard(invite.code)}
                          >
                            <Copy className="h-4 w-4" />
                          </Button>
                          <Button
                            size="icon"
                            variant="ghost"
                            onClick={() => deleteInvite(invite.code)}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}