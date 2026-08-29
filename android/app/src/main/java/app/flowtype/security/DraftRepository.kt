package app.flowtype.security

import app.flowtype.data.StorageDispatcher
import app.flowtype.session.ComputerSessions

class DraftRepository(
    private val store: SecureDraftStore,
    private val storage: StorageDispatcher,
) {
    private val loaded = mutableSetOf<String>()
    private val cache = mutableMapOf<String, ComputerSessions.ParkedSession>()

    @Synchronized
    fun load(pcId: String): ComputerSessions.ParkedSession? {
        if (pcId !in loaded) {
            store.load(pcId)?.let { cache[pcId] = it }
            loaded += pcId
        }
        return cache[pcId]
    }

    @Synchronized
    fun save(pcId: String, draft: ComputerSessions.ParkedSession) {
        loaded += pcId
        cache[pcId] = draft
        storage.execute(action = { store.save(pcId, draft) })
    }

    @Synchronized
    fun clear(pcId: String) {
        loaded += pcId
        cache.remove(pcId)
        storage.execute(action = { store.clear(pcId) })
    }

    fun preload(pcIds: Collection<String>, completion: () -> Unit) {
        storage.execute(
            action = { pcIds.forEach(::load) },
            completion = completion,
        )
    }
}
