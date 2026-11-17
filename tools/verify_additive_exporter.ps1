# Tie the archived converter binaries to the source branches used to interpret additive flags.
$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$archive = Join-Path $project 'docs/imported/fortnite-analysis/tools'
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Linq;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
public static class AdditivePdbProof {
 public static string[] Read(string pdb,string dll,string documentSuffix) {
  using var ps=File.OpenRead(pdb);
  using var provider=MetadataReaderProvider.FromPortablePdbStream(ps);
  var reader=provider.GetMetadataReader();
  using var ds=File.OpenRead(dll);using var pe=new PEReader(ds);
  var cv=pe.ReadCodeViewDebugDirectoryData(pe.ReadDebugDirectory().First(x=>x.Type==DebugDirectoryEntryType.CodeView));
  var pdbId=new Guid(reader.DebugMetadataHeader.Id.Take(16).ToArray());
  if(pdbId!=cv.Guid)throw new InvalidDataException("PDB does not match converter DLL");
  foreach(var handle in reader.Documents) {
   var doc=reader.GetDocument(handle);var name=reader.GetString(doc.Name);
   if(!name.EndsWith(documentSuffix))continue;
   if(reader.GetGuid(doc.HashAlgorithm).ToString()!="8829d00f-11b8-4213-878b-770e8597ac16")throw new InvalidDataException("Expected SHA-256 source document");
   return new[]{pdbId.ToString(),Convert.ToHexString(reader.GetBlobBytes(doc.Hash)).ToLowerInvariant()};
  }
  throw new InvalidDataException("Source document missing from PDB");
 }
}
'@
$binaryRoot = Join-Path $archive 'asset-converter/bin/Release/net10.0'
$documents = @(
    @('animConverter','CUE4Parse-Conversion','AnimConverter.cs','CUE4Parse/CUE4Parse-Conversion/Legacy/AnimConverter.cs'),
    @('ueAnim','CUE4Parse-Conversion','UEAnim.cs','CUE4Parse/CUE4Parse-Conversion/Writers/UEFormat/UEAnim.cs'),
    @('animSequence','CUE4Parse','UAnimSequence.cs','CUE4Parse/CUE4Parse/UE4/Assets/Exports/Animation/UAnimSequence.cs')
)
$sourceHashes = @{}
$binaryEvidence = @{}
foreach ($document in $documents) {
    $dll = Join-Path $binaryRoot ($document[1] + '.dll')
    $pdb = Join-Path $binaryRoot ($document[1] + '.pdb')
    $proof = [AdditivePdbProof]::Read($pdb,$dll,$document[2])
    $actual = (Get-FileHash -LiteralPath (Join-Path $archive $document[3]) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($proof[1] -ne $actual) { throw 'Source does not match the binary PDB' }
    $sourceHashes[$document[0]] = $actual
    $binaryEvidence[$document[1]] = @{ pdbGuid=$proof[0]; sha256=(Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$result = @{ converter='cue4parse-ueanim-v10'; pdbMatchesBinary=$true; sourceSha256=$sourceHashes; binaries=$binaryEvidence }
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $project 'docs/imported/additive-exporter-proof.json') -Encoding utf8
$result | ConvertTo-Json -Depth 6
