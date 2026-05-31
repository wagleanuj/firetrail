import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { EpicChips } from './epic-chips'

describe('EpicChips', () => {
  it('toggles an epic on click', () => {
    const onChange = vi.fn()
    render(<EpicChips epics={[{ id: 'EPIC-1', short_id: 'EPIC-1', title: 'Auth' }]} selected={new Set()} onChange={onChange} />)
    fireEvent.click(screen.getByText('Auth'))
    expect(onChange).toHaveBeenCalledWith(expect.any(Set))
  })
  it('renders a No epic chip', () => {
    render(<EpicChips epics={[]} selected={new Set()} onChange={() => {}} />)
    expect(screen.getByText(/no epic/i)).toBeInTheDocument()
  })
  it('reflects selected state', () => {
    render(<EpicChips epics={[{ id: 'EPIC-1', short_id: 'EPIC-1', title: 'Auth' }]} selected={new Set(['EPIC-1'])} onChange={() => {}} />)
    // the selected chip should carry aria-pressed=true
    expect(screen.getByRole('button', { name: /auth/i })).toHaveAttribute('aria-pressed', 'true')
  })
})
