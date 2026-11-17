# Conteúdo extraído no Fightnight

Pedido de 16/11/2026: importar o material extraído do Fortnite no Fightnight e
remover a antiga pasta `games/fightnight/fortnite` após conferir a migração.

## Organização

- `assets/imported/sources`: acesso às três árvores de arquivos convertidos,
  preservadas fisicamente em `docs/imported/fortnite-analysis/assets/converted`.
  Os vínculos apontam somente para caminhos atuais dentro do Fightnight.
- `assets/imported/files`: subconjunto de links físicos criado e conferido durante
  a importação inicial; preservado, sem duplicação dos bytes no disco.
- `assets/imported/catalog.json`: catálogo integral com procedência por arquivo.
- `dist/assets/imported`: biblioteca do build local, dividida por categoria,
  com vínculos para os arquivos preservados. Para distribuir o acervo em outro
  computador é preciso incluir os destinos desses vínculos; este trabalho não
  publicou nem enviou os assets a um serviço externo.
- `docs/imported/fortnite-analysis`: acervo de extração, pacotes brutos, metadados,
  evidências, ferramentas, referências e projeto de análise, preservados integralmente.
- `docs/imported/fortnite-webgpu`: protótipo anterior e seu histórico Git preservados.

Os assets extraídos continuam tendo a procedência original. Não fazem parte da
licença MIT do código do Fightnight nem são apresentados como modelos autorais.

## No jogo

O botão **Conteúdo importado** no menu e na pausa abre a biblioteca. As malhas GLB
podem ser colocadas como objetos decorativos em partidas solo ou usadas como a
aparência de armas. O fuzil `Assault_SunRose_Rifle` e a escopeta
`ThreeBarrel_Shotgun`, quando presentes no catálogo verificado, são carregados no
início da sessão. A simulação de combate continua sendo a do Fightnight.

Texturas e áudio compatível têm prévias; os arquivos originais podem ser baixados.
As categorias são carregadas sob demanda e a lista usa páginas de 30 recursos.
Até 16 objetos decorativos podem ser colocados na partida. São locais, sem colisão
de gameplay e não são replicados por rede. **Restaurar modelos do Fightnight**
remove as alterações visuais e libera as malhas importadas.

O renderizador atual usa cores de vértice e fatores de material. Texturas separadas
continuam no acervo; não há vínculo material-textura inventado pela proximidade dos
arquivos. A prévia cinza mostra a geometria real. Desde 17/11, animações UEAnim completas
podem mover os modelos com ossos correspondentes e ser colocadas como objetos animados
no cenário. Áudio RADA tem conversão local sob demanda e pode substituir efeitos da
partida. Veja [instruções, formatos e limites](native-media.md). Esqueletos UEFormat
isolados e animações aditivas continuam exigindo adaptação. Mapas completos e código original de gameplay não foram recuperados
por esta importação. A extração original é parcial, conforme seus próprios relatórios.

## Reprodução

```powershell
python tools/assetgen/catalog_preserved_source.py
python tools/assetgen/stage_imported.py
$env:WASM_BINDGEN = 'X:\tools\downloads\wasm-bindgen-0.2.129\wasm-bindgen-0.2.129-x86_64-pc-windows-msvc\wasm-bindgen.exe'
./scripts/build.ps1
```

O executável oficial wasm-bindgen 0.2.129 foi baixado isoladamente para corresponder
ao Cargo.lock, sem substituir a versão compartilhada 0.2.130. SHA-256 do pacote:
`79e348c169d0c10c0b287647f1e753d0985353ca96800bc29be8f4261baf13e6`.

O catálogo cobre os 7.321.963 registros do manifesto completo, incluindo retomadas,
e os sete arquivos de provas anteriores: **678.964 saídas, 632.347 hashes distintos,
62.931.721.584 bytes lógicos**. As identidades das pastas principais foram conferidas
antes/depois da migração. O primeiro comando PowerShell começou uma transferência
por arquivo; ela foi interrompida e reunida sem sobrescritas. A renomeação direta
preservou as raízes originais. As raízes principal e de provas mantiveram seus IDs;
os três payloads da prova de áudio/animação tiveram todos os SHA-256 reconferidos.

Não foi feito um novo hash de todos os 63 GB. As prévias de modelos, imagens, áudio
compatível e dados verificam o SHA-256 antes da decodificação; modelos incompatíveis
são recusados sem afetar o jogo. `import_extracted.py --from-index` permanece disponível
para uma auditoria completa por arquivo, que leva mais tempo. Arquivos locais gerados
e acervos volumosos ficam fora do Git.

## Evidências

- `import-verification.json`: cobertura do catálogo, identidades e limites da verificação.
- `move-reconciliation.json`: recuperação da transferência inicial e hashes das três provas.
- `source-bindings.json`: destinos atuais conferidos no build.
- `migration-verification.json`: movimento do acervo e remoção da pasta antiga.
- `browser-verification.json`: biblioteca e geometria exercitadas no navegador real.
- `library-browser.png` e `model-in-game.png`: capturas da validação.

As contagens finais e a conclusão da migração devem ser lidas nos relatórios acima.
