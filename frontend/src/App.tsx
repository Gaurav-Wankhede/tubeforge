import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Toaster } from 'react-hot-toast'
import Layout from './components/layout/Layout'
import AnalysisDashboard from './routes/AnalysisDashboard'
import TopicResearch from './routes/TopicResearch'
import NextVideo from './routes/NextVideo'
import KeywordOpportunity from './routes/KeywordOpportunity'
import TagIntelligence from './routes/TagIntelligence'
import Videos from './routes/Videos'
import Scores from './routes/Scores'
import ScoreDetail from './routes/ScoreDetail'
import Gaps from './routes/Gaps'
import Tags from './routes/Tags'
import Keywords from './routes/Keywords'
import Scorecard from './routes/Scorecard'
import Audit from './routes/Audit'
import Ideas from './routes/Ideas'
import Alerts from './routes/Alerts'
import Health from './routes/Health'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route element={<Layout />}>
            {/* Growth Command Center — analysis first, no raw data */}
            <Route path="/" element={<TopicResearch />} />
            <Route path="/analysis" element={<AnalysisDashboard />} />
            <Route path="/next-video" element={<NextVideo />} />
            <Route path="/opportunity" element={<KeywordOpportunity />} />
            <Route path="/tags-intel" element={<TagIntelligence />} />
            {/* Legacy raw-data views (secondary) */}
            <Route path="/videos" element={<Videos />} />
            <Route path="/scores" element={<Scores />} />
            <Route path="/scores/:id" element={<ScoreDetail />} />
            <Route path="/gaps" element={<Gaps />} />
            <Route path="/tags" element={<Tags />} />
            <Route path="/keywords" element={<Keywords />} />
            <Route path="/scorecard" element={<Scorecard />} />
            <Route path="/audit" element={<Audit />} />
            <Route path="/ideas" element={<Ideas />} />
            <Route path="/alerts" element={<Alerts />} />
            <Route path="/health" element={<Health />} />
          </Route>
        </Routes>
      </BrowserRouter>
      <Toaster
        position="bottom-right"
        toastOptions={{
          style: {
            background: '#1a1a1a',
            color: '#fff',
            border: '1px solid #333',
            borderRadius: 8,
          },
        }}
      />
    </QueryClientProvider>
  )
}
