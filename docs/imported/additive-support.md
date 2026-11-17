# Resolução das animações aditivas

## O que os arquivos realmente contêm

O campo `AdditiveAnimType` do UEAnim não determina sozinho como ler as trilhas.
O CUE4Parse arquivado chama `ConvertAdditive` quando `IsValidAdditive()` aceita a
referência, mas o escritor UEAnim mantém os campos da sequência original. Assim,
parte dos arquivos marcados como aditivos já contém poses completas.

Essa interpretação foi conferida contra os binários arquivados: os GUIDs dos
Portable PDBs coincidem com os das DLLs e os hashes SHA-256 dos documentos
`AnimConverter.cs`, `UEAnim.cs` e `UAnimSequence.cs` no PDB coincidem com os fontes
inspecionados. A prova está em `additive-exporter-proof.json` e pode ser refeita com:

```powershell
./tools/verify_additive_exporter.ps1
node tools/prepare_additive_registry.mjs
node --test tools/imported_additive.test.mjs
```

`GhostRock_Jog_N_CMM` é um caso diferente: additive=1, basePose=2, referência vazia,
posição e escala da pélvis iguais a zero. Os metadados originais completos também
não contêm `RefPoseSeq`. Esse arquivo contém deltas, porque o conversor não conseguiu
expandir a referência ausente. Somá-lo à malha como pose completa colapsaria o rig.

## Bases explícitas do Fightnight

O registro `additive-registry.json` contém 16 escolhas de adaptação:

- GhostRock_Jog, ExplosiveBow_Jog e ExplosiveBow_Relaxed_Jog, nas quatro direções
  N/S/E/W, usam os respectivos `_Core` **depois de verificar** que são poses completas.
- AssaultRifle_Idle_Patrol usa o quadro zero de AssaultRifle_Idle_Patrol_Pose.
- JumpUp/JumpFall/JumpLand_NoWep_CMM_Additive usam o quadro zero de NoWep_Jump_From_Idle.

Os hashes dos clipes e metadados, `SkeletonGuid`, `RetargetSource` e durações foram
conferidos. Cada vínculo declara `adaptedReference: true` e
`originalReferenceRecovered: false`: a referência ausente do Fortnite não foi
inventada nem apresentada como recuperada. Uma base escolhida explicitamente na
biblioteca usa o mesmo caminho e fica registrada como `user-selected-base`.

O sufixo `_Core` não é um critério de compatibilidade: por exemplo,
JumpLand_NoWep_CMM_Additive_Core também contém deltas. O registro não o utiliza.
GhostRock sul possui 41 quadros aditivos e 121 quadros-base na mesma duração de
quatro segundos; a composição reamostra pelo tempo, sem exigir FPS iguais.

## Composição

`createAdditiveResolver({ entries, loadClip, registry }).resolve(entry, options)`
retorna um clipe absoluto com o recibo `resolution`. `loadClip` deve conferir o
SHA-256 antes de chamar `parseUEAnim`. As opções são `model`, `baseEntry`, `weight`,
`baseMode` e `referenceFrame`.

Os nomes dos ossos são comparados sem distinguir maiúsculas e minúsculas, como
FName. Colisões são recusadas. As operações em coordenadas originais são:

- posição: base + peso × delta;
- escala: base × (1 + peso × delta);
- rotação local: quaternion delta ponderado × quaternion base;
- rotação em espaço da malha: converter as rotações-base para a hierarquia global,
  aplicar os deltas globais e voltar ao espaço local usando os pais já compostos.

Os modos de base aceitam a pose de referência, tempo proporcional ou quadro fixo.
Quaternions são normalizados; pesos usam interpolação pelo arco curto. Os clipes
originais nunca são alterados. Referências ausentes, ciclos, hashes divergentes,
hierarquias incompletas, pesos inválidos e escalas singulares são recusados.
Exportos já expandidos aceitam apenas peso um, pois não preservam os deltas
necessários para recalcular o peso. O adaptador não ignora esse controle em silêncio.

## Limite encontrado no próprio exportador

Alguns clipes de referência local (`basePose=4`) passaram pela expansão do
CUE4Parse, mas foram salvos com escalas zero. `AM_Chicken_Bump_Additive` é uma prova
real. O adaptador não os reinterpreta como deltas nem aplica uma segunda pose-base:
isso perderia a informação sobre a escala original. Esses arquivos precisam de
reconversão do conteúdo comprimido original e recebem uma mensagem específica.
Esse problema não afeta os 16 vínculos escolhidos para esta integração.

## Validação

```text
VERIFIED 16 explicit additive/base pairs; every source SHA-256 and metadata identity matched.
tests 9
pass 9
fail 0
skipped 0
```

Os testes exercitam local1 e meshspace2, ordem de quaternion, pesos zero/um,
reamostragem de FPS, preservação dos deltas, bases registradas, hashes, ciclos e
os casos reais GhostRock, AO_AshtonMilo_Targeted_CD, AS_BeanImpact_Additive_Neutral
e o exporto singular AM_Chicken_Bump_Additive. A validação visual do personagem
jogável é separada desses testes de formato e composição.
