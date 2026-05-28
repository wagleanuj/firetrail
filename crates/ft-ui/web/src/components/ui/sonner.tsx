import { Toaster as Sonner } from 'sonner'

export type ToasterProps = React.ComponentProps<typeof Sonner>

export const Toaster = (props: ToasterProps) => <Sonner theme="dark" position="bottom-right" {...props} />
