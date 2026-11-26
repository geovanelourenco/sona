/* Teste de integração do plugin "Sona" com o libobs real, sem interface.
 * Carrega o plugin, cria a fonte, liga a um canal de saída e mede o áudio que
 * chega ao pipeline do OBS (callback de captura da fonte).
 *   cc -I/usr/include/obs tests/obs/harness.c -lobs -lm -o harness
 *   ./harness target/release/libsona_obs.so 8
 * Saída: JSON com quadros recebidos e pico.
 */
#include <math.h>
#include <obs-module.h>
#include <obs.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static volatile float peak = 0.0f;
static volatile long frames_seen = 0;

static void capture(void *param, obs_source_t *source, const struct audio_data *audio, bool muted)
{
	(void)param;
	(void)source;
	(void)muted;
	const float *l = (const float *)audio->data[0];
	for (uint32_t i = 0; l && i < audio->frames; i++) {
		float v = fabsf(l[i]);
		if (v > peak)
			peak = v;
	}
	frames_seen += audio->frames;
}

int main(int argc, char **argv)
{
	if (argc < 3)
		return 1;
	if (!obs_startup("pt-BR", NULL, NULL))
		return 2;
	struct obs_audio_info ai = {.samples_per_sec = 48000, .speakers = SPEAKERS_STEREO};
	if (!obs_reset_audio(&ai))
		return 3;
	obs_module_t *mod = NULL;
	int r = obs_open_module(&mod, argv[1], NULL);
	if (r != MODULE_SUCCESS) {
		printf("{\"erro\":\"obs_open_module %d\"}\n", r);
		return 4;
	}
	if (!obs_init_module(mod)) {
		printf("{\"erro\":\"obs_init_module\"}\n");
		return 5;
	}
	obs_source_t *src = obs_source_create("sona_bridge_audio", "Sona", NULL, NULL);
	if (!src) {
		printf("{\"erro\":\"fonte não criada\"}\n");
		return 6;
	}
	const char *name = obs_source_get_display_name("sona_bridge_audio");
	obs_set_output_source(0, src);
	obs_source_add_audio_capture_callback(src, capture, NULL);
	sleep((unsigned)atoi(argv[2]));
	printf("{\"nome\":\"%s\",\"quadros\":%ld,\"pico\":%.4f}\n", name ? name : "", frames_seen, peak);
	obs_source_remove_audio_capture_callback(src, capture, NULL);
	obs_set_output_source(0, NULL);
	obs_source_release(src);
	obs_shutdown();
	return 0;
}
