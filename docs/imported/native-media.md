# Áudio e animação nativos — 17/11/2026

O Fightnight reproduz clipes UEAnim completos sobre malhas GLB com esqueleto e
converte áudio RADA no servidor local. Os originais do acervo permanecem intactos.

## Usar no jogo

1. Abra **Conteúdo importado**, busque `SK_M_MALE_Base` em **Modelos** e selecione a malha.
2. Mude para **Animações**, busque `DualWield_Idle_CMM` e selecione o clipe.
3. A prévia mostra a malha deformada pelas trilhas originais; pause ou clique para girar.
4. **Colocar animação no cenário** instala um objeto animado na partida solo.
5. Em **Áudio**, escolha um RADA, ouça a prévia e use **Usar este som na partida** para
   substituir o efeito selecionado. **Restaurar efeitos sintetizados** desfaz as trocas.

Fuzil, escopeta e confirmação do menu já usam sete amostras preparadas por padrão:
três `SunRose_Assault_Fire_1P_Close`, três `Shotgun_VividRazor_BreakBarrel_Fire_1P_Close`
e `fort_ui_confirm_02`. São associações aos eventos do Fightnight; não reproduzem a
mixagem, as camadas, as regras ou a sincronização original do Fortnite.

## Ferramentas locais

```powershell
./scripts/build-native-audio.ps1
python tools/assetgen/prepare_native_audio.py
python tools/serve_imported.py --port 5196
```

O primeiro comando compila nosso adaptador contra headers e biblioteca estática
da instalação local `X:\UE\UE_5.8`, com Visual Studio em `X:\vstudio`. Ambos os caminhos
podem ser passados por parâmetros. O executável fica em `target/native-audio` e não
é incluído no build web nem no Git. O adaptador chama a API instalada; o navegador
recebe apenas WAV. Nenhuma biblioteca ou implementação do SDK é redistribuída.

O servidor escuta somente em `127.0.0.1`. A rota `/api/imported/audio/{id}` aceita
IDs presentes no catálogo, confere o SHA-256 do original e usa um cache com hash
do WAV em `dist/assets/imported/audio-cache`. O JSON devolvido vincula os dois hashes.
Um servidor estático comum reproduz os sete WAVs preparados, mas não faz novas
conversões. Se `dist` for reconstruído do zero, execute a preparação novamente.
`scripts/build.ps1` prepara o banco quando o decodificador local já existe.

## Formatos e limites

- UEAnim: versão 10 sem compressão, atributos METADATA/TRACKS, nomes de ossos,
  posições, quaternions e escalas. Conversão de coordenadas conforme o escritor
  GLTF do CUE4Parse preservado no acervo: centímetros para metros, troca de Y/Z,
  sinal de W. Interpolação linear e slerp pelo arco curto. Curvas/morphs não aplicados.
- Pelo menos 80% dos ossos devem corresponder por nome, sem distinguir maiúsculas;
  isso não é retarget universal. Os aditivos usam o resolvedor documentado em
  `additive-support.md`, incluindo bases explícitas e peso ajustável. Matrizes
  explícitas exigem conversão; ossos ocultos sem escala não derrubam a prévia.
- Objetos animados são decorativos, sem colisão ou comportamento de jogador/NPC.
  O personagem controlado possui integração própria em `playable-character.md`.
  Para objetos decorativos, são até 16 objetos,
  120 quadros e 1 milhão de vértices por clipe, 64 MB por envio, com orçamento total
  de 2 milhões de vértices de objetos importados. Clipes limitados a 20 segundos.
  A taxa é reduzida para caber no orçamento; os quadros são preparados uma vez.
  O renderizador escolhe o quadro pelo tempo de simulação, respeitando a pausa.
- RADA: até 64 MB comprimidos, oito canais e 120 segundos. O adaptador interpreta
  os envelopes SEEK constantes/variáveis inseridos pelo streaming da Unreal,
  valida cada bloco e exige todos os frames e bytes declarados. Não busca assinaturas
  para contornar blocos corrompidos. WAV PCM16 preserva taxa e canais originais.
- OPUS e BINKA nativos, arquivos incompletos e outros formatos ainda não têm este
  adaptador. A biblioteca mantém o original disponível e explica a falha.
- A conversão é sob demanda: não foram convertidos todos os 158.245 RADA de uma vez.
  Texturas separadas e materiais continuam sujeitos aos limites da importação anterior.

## Verificação

Testes em `tools/imported_animation.test.mjs`, `tools/assetgen/test_native_audio.py`,
`crates/fn-core/src/imported_animation.rs` e `tools/native_media_e2e.mjs` exercitam
payloads reais, rejeição de corrupção, pausa, troca de quadros, áudio na WebAudio e
restauração dos efeitos. Resultados desta execução: `native-media-browser-verification.json`.
As sete conversões padrão e seus hashes estão em `native-audio-presets.json`.

Últimas verificações registradas, incluindo a integração do personagem:

```text
Rust core: test result: ok. 348 passed; 0 failed; 2 ignored; acervo real explícito: 1 passed
JavaScript: tests 34; pass 34; fail 0; skipped 0
Python/native decoder (validação anterior): Ran 4 tests — OK
Chrome/WebGPU/WebAudio: 20 PASS; no JavaScript or WebGPU validation errors
Chrome/WebGPU/personagem: 38 PASS; no JavaScript or WebGPU validation errors
HTTP local: 200 (página, módulo UEAnim, WASM e conversão RADA)
```

Correções durante a validação: tipos/includes do adaptador C++; caminhos longos do
Windows; envelopes SEEK presentes na extração; fixture inicialmente aditiva trocada
por um clipe completo, mantendo o teste de rejeição; espera do navegador passou a
observar o avanço real dos quadros em vez de presumir que o primeiro envio à GPU
termina em 350 ms. Não houve falhas restantes nas verificações acima.
