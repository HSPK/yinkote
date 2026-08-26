import { DetailPanel } from '../components/DetailPanel'
import { ItemTable } from '../components/ItemTable'
import { Sidebar } from '../components/Sidebar'

/** The three-pane workbench: collections, results, detail. */
export function LibraryPage() {
  return (
    <div className="workspace">
      <Sidebar />
      <ItemTable />
      <DetailPanel />
    </div>
  )
}
