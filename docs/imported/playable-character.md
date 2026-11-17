# Personagem jogável com rig nativo

O banco local aplica `SK_M_MALE_Base` ao ator controlado em partidas solo: 88 ossos,
6.551 vértices, pesos de skin originais e 25 estados. O carregamento ocorre durante
o boot; a biblioteca também permite **Usar como personagem jogável** em outro corpo
humano completo e compatível. **Restaurar personagem do Fightnight** desfaz apenas
essa troca. O modo multiplayer continua usando seus avatares sincronizados.

O conjunto cobre idle, corrida nas quatro direções, sprint, agachamento parado e
direcional, salto, queda, aterrissagem, mergulho, planador, recarga, picareta, emote,
morte, nado, idle armado, motorista e mira em pé/agachada. As associações estão em
`playable-clip-candidates.json`; os hashes e as bases usadas estão em
`playable-character-bank.json`. As poses-base ausentes do material extraído foram
substituídas por adaptações explícitas, descritas em `additive-support.md`.

## Comportamento

- A simulação escolhe os estados; não há relógio independente por input. Pausar
  interrompe animação e envio dos vértices. Reiniciar a partida preserva o modelo.
- O deslocamento/rotação do osso root fica na física do jogo. Os movimentos da
  pélvis e dos membros são preservados. Não recentramos os pés a cada quadro.
- As poses locais usam interpolação de posição/escala e slerp de quaternion, com
  transições suaves. Mira, recarga e picareta usam camadas da parte superior.
- IK de dois ossos conecta as mãos às armas, itens, construção e barra do planador.
  A mão livre da recarga mantém o clipe nativo. Câmera, colisão, tiro, munição e
  colheita continuam sendo calculados pela simulação do Fightnight.
- O motorista acompanha a mesma transformação de inclinação usada pela cabine.
- O renderizador atualiza somente os vértices do jogador na GPU. O banco guarda
  poses de ossos, não uma cópia inteira da malha para cada quadro.

## Preparação e limites

```powershell
node tools/prepare_playable_character.mjs
node --test tools/imported_character.test.mjs tools/imported_additive.test.mjs
$env:FN_CHARACTER_BANK = (Resolve-Path 'dist/assets/imported/character.fnchar').Path
cargo test --locked -p fn-core recovered_player_bank -- --ignored --nocapture
$env:CHROME_PATH = 'C:\Program Files\Google\Chrome\Application\chrome.exe'
$env:FN_FLAGS = 'native'
node tools/playable_character_e2e.mjs
```

Os builds PowerShell/bash preparam o banco quando a extração local está disponível.
`dist/assets/imported/character.fnchar` e os manifestos são gerados, conferidos por
SHA-256 e não versionados. Os originais permanecem intactos.

Um modelo aceito precisa ter geometria ponderada na cabeça e nos dois pés,
os 13 ossos de controle e pelo menos 80% dos nomes das trilhas correspondentes.
Roupas parciais, rigs ambíguos ou incompatíveis, matrizes não convertidas, dados
não finitos e pesos inválidos são recusados antes da instalação. O envio é atômico:
um arquivo ruim preserva o personagem anterior. Restaurar durante o preparo cancela
o resultado pendente.

O limite é de 50 mil vértices, 100 mil triângulos, 512 nós, 64 MB e até 300 amostras
por clipe. A escala normaliza o corpo para 1,78 m. A aparência ainda usa cores de
vértices/materiais: texturas separadas, morphs e curvas faciais não são aplicados.
Este é um conjunto adaptado para as ações do Fightnight, não o Animation Blueprint
original completo nem um retarget universal para todos os corpos do acervo.

As evidências da partida ficam em `playable-character-browser-verification.json`
e nas capturas `player-*.png`; a biblioteca e o áudio são exercitados separadamente
por `tools/native_media_e2e.mjs`.

## Resultado da validação em 17/11/2026

```text
Rust core: test result: ok. 348 passed; 0 failed; 2 ignored
Acervo real (teste explícito): test result: ok. 1 passed; 0 failed
JavaScript: tests 34; pass 34; fail 0; skipped 0
Chrome/WebGPU — jogador: 38 PASS; no JavaScript or WebGPU validation errors
Chrome/WebGPU/WebAudio — biblioteca: 20 PASS; no JavaScript or WebGPU validation errors
WASM release: Finished `release` profile [optimized]
HTTP localhost:5196: 200; banco com 8.180.156 bytes, 88 ossos e 25 estados
```

Os dois testes ignorados no conjunto Rust são o marathon preexistente e o teste
que exige o acervo local; este último foi executado explicitamente e passou.
As poses renderizadas mediram 1,217 m de altura agachado e 0,504 m de altura no
mergulho horizontal. A barra do planador e os grips da mira ficaram ao alcance
das mãos, com erro de posição inferior a 0,001 m nas verificações registradas.

Correções encontradas na revisão: nomes FName com caixa diferente; root motion
somado à física; consumíveis sem IK; camada de picareta aplicada a granadas;
roupas sem cabeça aceitas como corpo completo; restauração durante preparo;
inclinação do motorista e alvo da barra do planador. Na repetição do teste de
partida, um toque automatizado em Espaço terminou entre quadros e expirou a espera
pelo salto; o teste agora mantém a tecla até a simulação entrar no salto. A nova
execução passou. As capturas também esperam a transição de pose antes do readback.
