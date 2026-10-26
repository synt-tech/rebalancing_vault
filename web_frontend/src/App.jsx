import { Routes, Route } from 'react-router-dom'
import Layout from './components/Layout'
import ToastContainer from './components/Toast'
import TxModal from './components/TxModal'
import VaultList from './pages/VaultList'
import VaultDetail from './pages/VaultDetail'
import Portfolio from './pages/Portfolio'

export default function App() {
  return (
    <Layout>
      <Routes>
        <Route path="/" element={<VaultList />} />
        <Route path="/vault/:address" element={<VaultDetail />} />
        <Route path="/portfolio" element={<Portfolio />} />
      </Routes>
      <ToastContainer />
      <TxModal />
    </Layout>
  )
}
