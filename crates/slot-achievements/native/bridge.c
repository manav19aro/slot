#include "rc_runtime.h"
#include "rc_trigger.h"

/* Keep the Rust boundary opaque; inspect trigger states using the pinned headers. */
static void discard_event(const rc_runtime_event_t* event) { (void)event; }

void slot_rc_do_frame(rc_runtime_t* runtime, rc_runtime_read_memory_func_t read, void* userdata) {
    rc_runtime_do_frame(runtime, discard_event, read, userdata, NULL);
}

int slot_rc_is_triggered(rc_runtime_t* runtime, uint32_t id) {
    struct rc_trigger_t* trigger = rc_runtime_get_achievement(runtime, id);
    return trigger && trigger->state == RC_TRIGGER_STATE_TRIGGERED;
}

#include "rc_hash.h"
#include "rc_consoles.h"

int slot_rc_gba_hash(char hash[33], const uint8_t* data, size_t size) {
    rc_hash_iterator_t iterator;
    rc_hash_initialize_iterator(&iterator, "game.gba", data, size);
    int result = rc_hash_generate(hash, RC_CONSOLE_GAMEBOY_ADVANCE, &iterator);
    rc_hash_destroy_iterator(&iterator);
    return result;
}

#include "rcheevos/src/util/md5.h"
/* Digest helper for the documented rcheevos award request signature. */
void slot_rc_md5(const uint8_t* data, int size, uint8_t digest[16]) {
    md5_state_t state;
    md5_init(&state);
    md5_append(&state, data, size);
    md5_finish(&state, digest);
}
