import type { ReactNode } from 'react'
import { X } from 'lucide-react'
import { Button } from './ui/button.js'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from './ui/dialog.js'
import { ScrollArea } from './ui/scroll-area.js'

/**
 * Relay's settings composition uses shadcn's Dialog and ScrollArea unchanged.
 * The frame only supplies desktop layout: a compact header, a non-scrolling
 * navigation column, and a detail pane that scrolls only when it has to.
 */
export function SettingsFrame({
  open,
  onOpenChange,
  title,
  description,
  closeLabel,
  navigation,
  children,
}: {
  open: boolean
  onOpenChange(open: boolean): void
  title: string
  description: string
  closeLabel: string
  navigation: ReactNode
  children: ReactNode
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="flex min-h-[22rem] max-h-[calc(100dvh-3rem)] w-full max-w-3xl flex-col gap-0 overflow-hidden p-0 sm:max-w-3xl"
      >
        <DialogHeader className="flex shrink-0 flex-row items-center justify-between gap-3 border-b px-4 py-3">
          <div className="min-w-0">
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription className="sr-only">{description}</DialogDescription>
          </div>
          <Button variant="ghost" size="icon-xs" aria-label={closeLabel} onClick={() => onOpenChange(false)}>
            <X data-icon="inline-start" />
          </Button>
        </DialogHeader>
        <div className="grid min-h-0 flex-1 grid-cols-[11rem_minmax(0,1fr)]">
          <nav className="min-h-0 border-r bg-muted/30 p-2">
            <ScrollArea className="h-full">{navigation}</ScrollArea>
          </nav>
          <ScrollArea className="min-h-0">{children}</ScrollArea>
        </div>
      </DialogContent>
    </Dialog>
  )
}
