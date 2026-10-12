import { useToast } from '@nuxt/ui/composables/useToast'

export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
export function useNotifications() {
  const toast = useToast()
  return function notify(message: string, error = false) {
    const duration = error ? 8000 : 3500
    const expiresAt = Date.now() + duration
    const notification = toast.add({
      title: message,
      color: error ? 'error' : 'success',
      icon: error ? 'i-lucide-circle-alert' : 'i-lucide-check',
      // Own the deadline: Reka's built-in timer pauses when the window loses focus.
      duration: 0,
      progress: false,
      'onUpdate:open': (open) => {
        if (!open) cleanup()
      },
    })
    const timer = window.setTimeout(dismiss, duration)
    window.addEventListener('focus', expireIfDue)
    document.addEventListener('visibilitychange', expireIfDue)

    function cleanup() {
      window.clearTimeout(timer)
      window.removeEventListener('focus', expireIfDue)
      document.removeEventListener('visibilitychange', expireIfDue)
    }
    function dismiss() {
      cleanup()
      toast.remove(notification.id)
    }
    function expireIfDue() {
      // Hidden WebViews can suspend timers; dismiss expired messages on return.
      if (Date.now() >= expiresAt) dismiss()
    }
  }
}
