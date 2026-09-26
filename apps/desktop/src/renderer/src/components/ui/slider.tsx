import * as React from 'react'
import { Slider as SliderPrimitive } from 'radix-ui'
import { cn } from '../../lib/utils.js'

/** A small shadcn-style wrapper around Radix Slider for discrete runtime values. */
function Slider({ className, ...props }: React.ComponentProps<typeof SliderPrimitive.Root>) {
  return (
    <SliderPrimitive.Root
      data-slot="slider"
      className={cn('relative flex w-full touch-none items-center select-none data-[disabled]:opacity-50', className)}
      {...props}
    >
      <SliderPrimitive.Track className="relative h-1.5 w-full grow overflow-hidden rounded-full bg-input">
        <SliderPrimitive.Range className="absolute h-full bg-primary" />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb className="block size-4 rounded-full border border-primary bg-background shadow-xs outline-none transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none" />
    </SliderPrimitive.Root>
  )
}

export { Slider }
