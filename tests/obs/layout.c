/* Confere que as estruturas espelhadas em apps/obs-plugin/src/lib.rs batem
 * com os cabeçalhos oficiais do libobs. Compila só se bater:
 *   cc -I/usr/include/obs -fsyntax-only tests/obs/layout.c
 */
#include <stddef.h>
#include <obs-module.h>

_Static_assert(offsetof(struct obs_source_info, id) == 0, "id");
_Static_assert(offsetof(struct obs_source_info, type) == 8, "type");
_Static_assert(offsetof(struct obs_source_info, output_flags) == 12, "output_flags");
_Static_assert(offsetof(struct obs_source_info, get_name) == 16, "get_name");
_Static_assert(offsetof(struct obs_source_info, create) == 24, "create");
_Static_assert(offsetof(struct obs_source_info, destroy) == 32, "destroy");

_Static_assert(offsetof(struct obs_source_audio, data) == 0, "data");
_Static_assert(offsetof(struct obs_source_audio, frames) == 64, "frames");
_Static_assert(offsetof(struct obs_source_audio, speakers) == 68, "speakers");
_Static_assert(offsetof(struct obs_source_audio, format) == 72, "format");
_Static_assert(offsetof(struct obs_source_audio, samples_per_sec) == 76, "samples_per_sec");
_Static_assert(offsetof(struct obs_source_audio, timestamp) == 80, "timestamp");
_Static_assert(sizeof(struct obs_source_audio) == 88, "sizeof obs_source_audio");

_Static_assert(OBS_SOURCE_TYPE_INPUT == 0, "tipo");
_Static_assert(OBS_SOURCE_AUDIO == (1 << 1), "flag");
_Static_assert(SPEAKERS_STEREO == 2, "estéreo");
_Static_assert(AUDIO_FORMAT_FLOAT_PLANAR == 8, "float planar");
_Static_assert(MAX_AV_PLANES == 8, "planos");
