import { useState } from 'react'
import { Outlet, NavLink } from 'react-router-dom'
import {
  Target,
  Key,
  Tags,
  Film,
  Trophy,
  Lightbulb,
  Activity,
  Bell,
  Sun,
  Moon,
  Menu,
  X,
  Radar,
  ClipboardCheck,
  Sparkles,
  Search,
} from 'lucide-react'
import { useTheme } from '../../lib/hooks'
import { ConnectionStatus } from '../ConnectionStatus'

const nav = [
  // Growth Command Center — analysis first, no raw data
  { to: '/', icon: Search, label: 'Topic Research' },
  { to: '/analysis', icon: Sparkles, label: 'Command Center' },
  { to: '/next-video', icon: Target, label: 'Next Video' },
  { to: '/opportunity', icon: Key, label: 'Keyword Opportunity' },
  { to: '/tags-intel', icon: Tags, label: 'Tag Intelligence' },
]

const dataNav = [
  { to: '/videos', icon: Film, label: 'Videos' },
  { to: '/scorecard', icon: Trophy, label: 'Scorecard' },
  { to: '/audit', icon: ClipboardCheck, label: 'Audit' },
  { to: '/gaps', icon: Radar, label: 'Gaps' },
  { to: '/keywords', icon: Key, label: 'Keywords' },
  { to: '/tags', icon: Tags, label: 'Tags' },
  { to: '/ideas', icon: Lightbulb, label: 'Ideas' },
  { to: '/alerts', icon: Bell, label: 'Alerts' },
  { to: '/health', icon: Activity, label: 'Health' },
]

export default function Layout() {
  const { theme, toggle } = useTheme()
  const [collapsed, setCollapsed] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)

  return (
    <div className="flex h-screen overflow-hidden">
      {/* Mobile overlay */}
      {mobileOpen && (
        <div
          className="fixed inset-0 z-40 bg-black/50 lg:hidden"
          onClick={() => setMobileOpen(false)}
        />
      )}

      {/* Sidebar */}
      <aside
        className={`fixed z-50 lg:static inset-y-0 left-0 flex flex-col bg-sidebar border-r border-border transition-all duration-200 ${
          collapsed ? 'w-16' : 'w-56'
        } ${mobileOpen ? 'translate-x-0' : '-translate-x-full lg:translate-x-0'}`}
      >
        {/* Logo */}
        <div className="flex items-center justify-between h-14 px-3 border-b border-border">
          {!collapsed && (
            <span className="text-lg font-bold bg-gradient-to-r from-blue-400 to-purple-400 bg-clip-text text-transparent">
              TubeForge
            </span>
          )}
          <button
            onClick={() => setCollapsed(!collapsed)}
            className="hidden lg:block p-1 rounded hover:bg-surface-hover"
          >
            {collapsed ? <Menu size={18} /> : <X size={18} />}
          </button>
        </div>

        {/* Nav */}
        <nav className="flex-1 py-2 overflow-y-auto">
          <div className="px-3 pb-1 text-[10px] uppercase tracking-wider text-gray-600">Growth</div>
          {nav.map(({ to, icon: Icon, label }) => (
            <NavLink
              key={to}
              to={to}
              onClick={() => setMobileOpen(false)}
              className={({ isActive }) =>
                `flex items-center gap-3 px-3 py-2.5 mx-2 rounded-lg text-sm transition-colors ${
                  isActive
                    ? 'bg-accent/15 text-accent font-medium'
                    : 'text-gray-400 hover:text-gray-200 hover:bg-surface-hover'
                }`
              }
            >
              <Icon size={18} />
              {!collapsed && <span>{label}</span>}
            </NavLink>
          ))}
          <div className="px-3 pt-4 pb-1 text-[10px] uppercase tracking-wider text-gray-600">Data</div>
          {dataNav.map(({ to, icon: Icon, label }) => (
            <NavLink
              key={to}
              to={to}
              onClick={() => setMobileOpen(false)}
              className={({ isActive }) =>
                `flex items-center gap-3 px-3 py-2.5 mx-2 rounded-lg text-sm transition-colors ${
                  isActive
                    ? 'bg-accent/15 text-accent font-medium'
                    : 'text-gray-400 hover:text-gray-200 hover:bg-surface-hover'
                }`
              }
            >
              <Icon size={18} />
              {!collapsed && <span>{label}</span>}
            </NavLink>
          ))}
        </nav>

        {/* Alerts link + theme */}
        <div className="border-t border-border p-2 space-y-1">
          <div className="flex items-center justify-between px-3 py-2">
            {!collapsed && <span className="text-[10px] uppercase tracking-wider text-gray-600">Status</span>}
            <ConnectionStatus />
          </div>
          <button
            onClick={toggle}
            className="flex items-center gap-3 px-3 py-2.5 w-full rounded-lg text-sm text-gray-400 hover:text-gray-200 hover:bg-surface-hover transition-colors"
          >
            {theme === 'dark' ? <Sun size={18} /> : <Moon size={18} />}
            {!collapsed && <span>{theme === 'dark' ? 'Light' : 'Dark'}</span>}
          </button>
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-y-auto">
        {/* Mobile header */}
        <div className="lg:hidden flex items-center h-14 px-4 border-b border-border bg-sidebar">
          <button onClick={() => setMobileOpen(true)} className="p-1">
            <Menu size={20} />
          </button>
          <span className="ml-3 text-lg font-bold bg-gradient-to-r from-blue-400 to-purple-400 bg-clip-text text-transparent">
            TubeForge
          </span>
        </div>

        <div className="p-4 lg:p-6">
          <Outlet />
        </div>
      </main>
    </div>
  )
}
