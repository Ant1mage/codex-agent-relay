import { Check } from 'lucide-react'
import { cn } from '../lib/utils.js'

/**
 * A Relay-owned composition for the one place shadcn has no primitive: a
 * non-clickable setup progress indicator. It deliberately communicates status
 * rather than pretending the setup pages are freely navigable tabs.
 */
export function OnboardingStepper({
  current,
  labels,
}: {
  current: number
  labels: readonly string[]
}) {
  return (
    <ol className="grid grid-cols-3 gap-1 rounded-lg border bg-muted/30 p-1.5" aria-label={labels.join(' · ')}>
      {labels.map((label, index) => {
        const complete = index < current
        const active = index === current
        return (
          <li
            className={cn(
              'flex min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-xs font-medium text-muted-foreground',
              active && 'bg-background text-foreground shadow-xs',
              complete && 'text-foreground',
            )}
            key={label}
          >
            <span
              className={cn(
                'grid size-4 shrink-0 place-items-center rounded-full border text-[10px]',
                complete && 'border-primary bg-primary text-primary-foreground',
                active && 'border-primary text-primary',
              )}
              aria-hidden="true"
            >
              {complete ? <Check className="size-3" /> : index + 1}
            </span>
            <span className="truncate">{label}</span>
          </li>
        )
      })}
    </ol>
  )
}
