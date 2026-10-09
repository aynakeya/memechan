import { useToast } from '@nuxt/ui/composables/useToast'

export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
export function useNotifications() {
  const toast = useToast()
  return function notify(message: string, error = false) {
    toast.add({
      title: message,
      color: error ? 'error' : 'success',
      icon: error ? 'i-lucide-circle-alert' : 'i-lucide-check',
      duration: error ? 8000 : 3500,
    })
  }
}
