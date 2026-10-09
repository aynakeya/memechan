export const sizeLabel = (size: number) => (size ? `最长边 ${size} px` : '原始尺寸')
export const bytesLabel = (bytes: number) =>
  bytes < 1048576 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1048576).toFixed(1)} MB`
export const stateLabel = (state: string) =>
  ({ ready: '已完成', pending: '待处理', error: '处理失败', idle: '未生成' })[state] ?? state
