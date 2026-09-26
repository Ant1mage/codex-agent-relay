import { Button } from './ui/button.js'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from './ui/dialog.js'

export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  cancelLabel,
  confirmLabel,
  onConfirm,
}: {
  open: boolean
  onOpenChange(open: boolean): void
  title: string
  description: string
  cancelLabel: string
  confirmLabel: string
  onConfirm(): void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[360px] gap-3 p-4" showCloseButton={false}>
        <DialogHeader className="gap-1 text-left">
          <DialogTitle className="text-sm">{title}</DialogTitle>
          <DialogDescription className="text-xs leading-relaxed">{description}</DialogDescription>
        </DialogHeader>
        <DialogFooter className="flex-row justify-end gap-2">
          <Button size="sm" variant="outline" onClick={() => onOpenChange(false)}>
            {cancelLabel}
          </Button>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => {
              onOpenChange(false)
              onConfirm()
            }}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
