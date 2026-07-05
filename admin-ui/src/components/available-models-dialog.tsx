import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Badge } from '@/components/ui/badge'

export interface AvailableModelInfo {
  id: string
  displayName: string
  multiplier: number
  variant: 'base' | 'thinking' | 'agentic'
}

export const AVAILABLE_MODELS: AvailableModelInfo[] = [
  { id: 'claude-sonnet-5', displayName: 'Claude Sonnet 5', multiplier: 1.3, variant: 'base' },
  { id: 'claude-sonnet-5-thinking', displayName: 'Claude Sonnet 5 (Thinking)', multiplier: 1.3, variant: 'thinking' },
  { id: 'claude-sonnet-5-agentic', displayName: 'Claude Sonnet 5 (Agentic)', multiplier: 1.3, variant: 'agentic' },
  { id: 'claude-sonnet-4-6', displayName: 'Claude Sonnet 4.6', multiplier: 1.3, variant: 'base' },
  { id: 'claude-sonnet-4-6-thinking', displayName: 'Claude Sonnet 4.6 (Thinking)', multiplier: 1.3, variant: 'thinking' },
  { id: 'claude-sonnet-4-6-agentic', displayName: 'Claude Sonnet 4.6 (Agentic)', multiplier: 1.3, variant: 'agentic' },
  { id: 'claude-sonnet-4-5-20250929', displayName: 'Claude Sonnet 4.5', multiplier: 1.3, variant: 'base' },
  { id: 'claude-sonnet-4-5-20250929-thinking', displayName: 'Claude Sonnet 4.5 (Thinking)', multiplier: 1.3, variant: 'thinking' },
  { id: 'claude-sonnet-4-5-20250929-agentic', displayName: 'Claude Sonnet 4.5 (Agentic)', multiplier: 1.3, variant: 'agentic' },
  { id: 'claude-opus-4-5-20251101', displayName: 'Claude Opus 4.5', multiplier: 2.2, variant: 'base' },
  { id: 'claude-opus-4-5-20251101-thinking', displayName: 'Claude Opus 4.5 (Thinking)', multiplier: 2.2, variant: 'thinking' },
  { id: 'claude-opus-4-5-20251101-agentic', displayName: 'Claude Opus 4.5 (Agentic)', multiplier: 2.2, variant: 'agentic' },
  { id: 'claude-opus-4-6', displayName: 'Claude Opus 4.6', multiplier: 2.2, variant: 'base' },
  { id: 'claude-opus-4-6-thinking', displayName: 'Claude Opus 4.6 (Thinking)', multiplier: 2.2, variant: 'thinking' },
  { id: 'claude-opus-4-6-agentic', displayName: 'Claude Opus 4.6 (Agentic)', multiplier: 2.2, variant: 'agentic' },
  { id: 'claude-opus-4-7', displayName: 'Claude Opus 4.7', multiplier: 2.2, variant: 'base' },
  { id: 'claude-opus-4-7-thinking', displayName: 'Claude Opus 4.7 (Thinking)', multiplier: 2.2, variant: 'thinking' },
  { id: 'claude-opus-4-7-agentic', displayName: 'Claude Opus 4.7 (Agentic)', multiplier: 2.2, variant: 'agentic' },
  { id: 'claude-opus-4-8', displayName: 'Claude Opus 4.8', multiplier: 2.2, variant: 'base' },
  { id: 'claude-opus-4-8-thinking', displayName: 'Claude Opus 4.8 (Thinking)', multiplier: 2.2, variant: 'thinking' },
  { id: 'claude-opus-4-8-agentic', displayName: 'Claude Opus 4.8 (Agentic)', multiplier: 2.2, variant: 'agentic' },
  { id: 'claude-haiku-4-5-20251001', displayName: 'Claude Haiku 4.5', multiplier: 0.5, variant: 'base' },
  { id: 'claude-haiku-4-5-20251001-thinking', displayName: 'Claude Haiku 4.5 (Thinking)', multiplier: 0.5, variant: 'thinking' },
  { id: 'claude-haiku-4-5-20251001-agentic', displayName: 'Claude Haiku 4.5 (Agentic)', multiplier: 0.5, variant: 'agentic' },
]

interface AvailableModelsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  availableModelIds: string[]
}

function variantLabel(variant: AvailableModelInfo['variant']): string {
  switch (variant) {
    case 'thinking':
      return 'Thinking'
    case 'agentic':
      return 'Agentic'
    case 'base':
      return '基础'
  }
}

function modelInfoForId(id: string): AvailableModelInfo {
  return AVAILABLE_MODELS.find((model) => model.id === id) ?? {
    id,
    displayName: id,
    multiplier: 1,
    variant: 'base',
  }
}

export function AvailableModelsDialog({
  open,
  onOpenChange,
  availableModelIds,
}: AvailableModelsDialogProps) {
  const availableIdSet = new Set(availableModelIds)
  const catalogIdSet = new Set(AVAILABLE_MODELS.map((model) => model.id))
  const knownModels = AVAILABLE_MODELS.filter((model) => availableIdSet.has(model.id))
  const unknownModels = availableModelIds
    .filter((id) => !catalogIdSet.has(id))
    .map(modelInfoForId)
  const models = [...knownModels, ...unknownModels]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>当前可用模型 ({models.length})</DialogTitle>
        </DialogHeader>

        <div className="max-h-[60vh] overflow-y-auto rounded-md border">
          {models.length > 0 ? (
            models.map((model) => (
              <div
                key={model.id}
                className="flex items-center justify-between gap-3 border-b px-3 py-2 last:border-b-0"
              >
                <div className="min-w-0">
                  <div className="text-sm font-medium">{model.displayName}</div>
                  <div className="truncate text-xs text-muted-foreground">{model.id}</div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <Badge variant="secondary">{variantLabel(model.variant)}</Badge>
                  <span className="text-xs text-muted-foreground">{model.multiplier}x</span>
                </div>
              </div>
            ))
          ) : (
            <div className="px-3 py-8 text-center text-sm text-muted-foreground">
              当前没有可用模型
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
