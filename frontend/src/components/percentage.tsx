import type { ComponentProps } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

export function Percentage({ value, children = `${Math.round(value)}%`, ...props }: ComponentProps<'span'> & {
  value: number
}) {
  return (
    <Tooltip>
      <TooltipTrigger delay={300} render={<span {...props} />} tabIndex={0}>
        {children}
      </TooltipTrigger>
      <TooltipContent>{value}%</TooltipContent>
    </Tooltip>
  )
}
