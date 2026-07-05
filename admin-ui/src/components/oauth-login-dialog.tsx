import { useMemo, useState } from 'react'
import { Copy, ExternalLink, Loader2, ShieldCheck } from 'lucide-react'
import { toast } from 'sonner'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  useCancelOAuthLogin,
  useCompleteOAuthLogin,
  useStartOAuthLogin,
} from '@/hooks/use-credentials'
import { extractErrorMessage } from '@/lib/utils'
import type { OAuthProvider, OAuthStartResponse } from '@/types/api'

interface OAuthLoginDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const DEFAULT_PROVIDER: OAuthProvider = 'Google'
const DEFAULT_REGION = 'us-east-1'

const PROVIDERS: Array<{
  id: OAuthProvider
  label: string
  authMethod: 'social' | 'idc'
}> = [
  { id: 'Google', label: 'Google', authMethod: 'social' },
  { id: 'Github', label: 'Github', authMethod: 'social' },
  { id: 'BuilderId', label: 'Builder ID', authMethod: 'idc' },
  { id: 'Enterprise', label: 'Enterprise', authMethod: 'idc' },
]

function isValidEnterpriseStartUrl(value: string): boolean {
  try {
    const parsed = new URL(value)
    return (
      parsed.protocol === 'https:' &&
      parsed.hostname.length > 0 &&
      parsed.pathname === '/start'
    )
  } catch {
    return false
  }
}

export function OAuthLoginDialog({ open, onOpenChange }: OAuthLoginDialogProps) {
  const [provider, setProvider] = useState<OAuthProvider>(DEFAULT_PROVIDER)
  const [region, setRegion] = useState(DEFAULT_REGION)
  const [startUrl, setStartUrl] = useState('')
  const [priority, setPriority] = useState('0')
  const [callbackUrl, setCallbackUrl] = useState('')
  const [session, setSession] = useState<OAuthStartResponse | null>(null)

  const startOAuth = useStartOAuthLogin()
  const completeOAuth = useCompleteOAuthLogin()
  const cancelOAuth = useCancelOAuthLogin()

  const selectedProvider = useMemo(
    () => PROVIDERS.find((item) => item.id === provider) ?? PROVIDERS[0],
    [provider]
  )
  const isEnterprise = provider === 'Enterprise'
  const hasStarted = Boolean(session)
  const isBusy =
    startOAuth.isPending || completeOAuth.isPending || cancelOAuth.isPending

  const reset = () => {
    setProvider(DEFAULT_PROVIDER)
    setRegion(DEFAULT_REGION)
    setStartUrl('')
    setPriority('0')
    setCallbackUrl('')
    setSession(null)
  }

  const handleClose = (options?: { completed?: boolean }) => {
    if (isBusy && !options?.completed) {
      return
    }

    const activeSessionId = session?.sessionId
    const shouldCancel = Boolean(activeSessionId && !options?.completed)

    if (shouldCancel && activeSessionId) {
      cancelOAuth.mutate(activeSessionId, {
        onError: (error: unknown) => {
          toast.error(`取消 OAuth 登录失败: ${extractErrorMessage(error)}`)
        },
      })
    }

    reset()
    onOpenChange(false)
  }

  const handleDialogOpenChange = (nextOpen: boolean) => {
    if (isBusy) {
      return
    }

    if (nextOpen) {
      onOpenChange(true)
      return
    }

    handleClose()
  }

  const openAuthPage = (authUrl: string) => {
    const popup = window.open(authUrl, '_blank', 'noopener,noreferrer')
    if (popup) {
      toast.success('已打开授权页面')
      return
    }

    toast.info('浏览器阻止了弹窗，请复制授权链接后手动打开')
  }

  const handleStart = () => {
    if (isEnterprise && !startUrl.trim()) {
      toast.error('请输入 Enterprise Start URL')
      return
    }

    if (isEnterprise && !isValidEnterpriseStartUrl(startUrl.trim())) {
      toast.error('Enterprise Start URL 必须是 https 链接，且路径为 /start')
      return
    }

    startOAuth.mutate(
      {
        provider,
        region: region.trim() || DEFAULT_REGION,
        startUrl: isEnterprise ? startUrl.trim() : null,
        priority: Number.parseInt(priority, 10) || 0,
      },
      {
        onSuccess: (data) => {
          setSession(data)
          setCallbackUrl('')
          openAuthPage(data.authUrl)
        },
        onError: (error: unknown) => {
          toast.error(`启动 OAuth 登录失败: ${extractErrorMessage(error)}`)
        },
      }
    )
  }

  const handleCopyAuthUrl = async () => {
    if (!session?.authUrl) {
      return
    }

    try {
      await navigator.clipboard.writeText(session.authUrl)
      toast.success('授权链接已复制')
    } catch (error) {
      toast.error(`复制授权链接失败: ${extractErrorMessage(error)}`)
    }
  }

  const handleComplete = () => {
    if (!session) {
      toast.error('请先开始 OAuth 登录')
      return
    }

    if (!callbackUrl.trim()) {
      toast.error('请粘贴回调 URL')
      return
    }

    completeOAuth.mutate(
      {
        sessionId: session.sessionId,
        callbackUrl: callbackUrl.trim(),
      },
      {
        onSuccess: (data) => {
          toast.success(`OAuth 凭据添加成功，ID: ${data.credentialId}`)
          handleClose({ completed: true })
        },
        onError: (error: unknown) => {
          toast.error(`完成 OAuth 登录失败: ${extractErrorMessage(error)}`)
        },
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={handleDialogOpenChange}>
      <DialogContent className="sm:max-w-xl max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>OAuth 登录</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-4 overflow-y-auto pr-1">
          <div className="space-y-2">
            <label className="text-sm font-medium">Provider</label>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              {PROVIDERS.map((item) => (
                <Button
                  key={item.id}
                  type="button"
                  variant={provider === item.id ? 'default' : 'outline'}
                  onClick={() => setProvider(item.id)}
                  disabled={isBusy || hasStarted}
                >
                  {item.label}
                </Button>
              ))}
            </div>
            <p className="text-xs text-muted-foreground">
              当前授权类型: {selectedProvider.authMethod === 'social' ? 'Social' : 'IdC'}
            </p>
          </div>

          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <div className="space-y-2">
              <label htmlFor="oauth-region" className="text-sm font-medium">
                Region
              </label>
              <Input
                id="oauth-region"
                placeholder={DEFAULT_REGION}
                value={region}
                onChange={(event) => setRegion(event.target.value)}
                disabled={isBusy || hasStarted}
              />
            </div>
            <div className="space-y-2">
              <label htmlFor="oauth-priority" className="text-sm font-medium">
                优先级
              </label>
              <Input
                id="oauth-priority"
                type="number"
                min="0"
                placeholder="0"
                value={priority}
                onChange={(event) => setPriority(event.target.value)}
                disabled={isBusy || hasStarted}
              />
            </div>
          </div>

          {isEnterprise ? (
            <div className="space-y-2">
              <label htmlFor="oauth-start-url" className="text-sm font-medium">
                Enterprise Start URL
              </label>
              <Input
                id="oauth-start-url"
                placeholder="https://d-xxxxxxxxxx.awsapps.com/start"
                value={startUrl}
                onChange={(event) => setStartUrl(event.target.value)}
                disabled={isBusy || hasStarted}
              />
              <p className="text-xs text-muted-foreground">
                Enterprise 需要粘贴 AWS IAM Identity Center 的 start URL。
              </p>
            </div>
          ) : null}

          {session ? (
            <div className="space-y-3 rounded-md border p-3">
              <div className="flex items-center gap-2 text-sm font-medium">
                <ShieldCheck className="h-4 w-4" />
                {selectedProvider.label} 授权已开始
              </div>
              <div className="rounded-md bg-muted/50 p-3 text-sm">
                <div className="font-medium">完成方式</div>
                <p className="mt-1 text-muted-foreground">
                  浏览器完成授权后，把最终回调 URL 完整粘贴回这里提交。
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => openAuthPage(session.authUrl)}
                  disabled={isBusy}
                >
                  <ExternalLink className="h-4 w-4" />
                  打开授权页
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleCopyAuthUrl}
                  disabled={isBusy}
                >
                  <Copy className="h-4 w-4" />
                  复制授权链接
                </Button>
              </div>
              <div className="space-y-2">
                <label htmlFor="oauth-callback-url" className="text-sm font-medium">
                  Callback URL
                </label>
                <Input
                  id="oauth-callback-url"
                  placeholder="粘贴授权完成后的 callback URL"
                  value={callbackUrl}
                  onChange={(event) => setCallbackUrl(event.target.value)}
                  autoComplete="off"
                  autoCorrect="off"
                  spellCheck={false}
                  disabled={completeOAuth.isPending}
                />
              </div>
            </div>
          ) : (
            <div className="rounded-md border p-3 text-sm text-muted-foreground">
              点击开始授权后会打开 OAuth 页面。授权完成后，仅需把 callback URL 粘贴回来提交。
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => handleClose()}
            disabled={isBusy}
          >
            取消
          </Button>
          {session ? (
            <Button type="button" onClick={handleComplete} disabled={isBusy}>
              {completeOAuth.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : null}
              提交 Callback URL
            </Button>
          ) : (
            <Button type="button" onClick={handleStart} disabled={isBusy}>
              {startOAuth.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : null}
              开始授权
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
