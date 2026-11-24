# Product

## Platform

web (console) + servidor nativo (Rust)

## Users

Operadores de som de igrejas, muitas vezes voluntários, mixando a transmissão ao
vivo durante o culto em paralelo à mesa física do PA. Em volta deles: quem opera
a projeção (Lumo) e a transmissão (OBS), e o técnico responsável pela mesa.

## Product Purpose

Fazer um mix de transmissão independente dos canais da mesa física, receber a
mídia dos computadores e devolver para a mesa e para o OBS. Sucesso: a
transmissão soa bem sem mexer no PA, e uma falha do Sona nunca derruba o PA.

## Operating Context

Culto ao vivo, sala escura, sob pressão. Ui24R ligada por USB a um servidor
local. Operadores em navegador (notebook, tablet). Rede local da igreja, às
vezes sem internet.

## Brand Commitments

Identidade do Lumoverso, a mesma do Lumo: fundo obsidiana, violeta elétrico como
acento primário, ciano aurora como segundo acento. Vermelho só para "no ar" e
para clip do medidor. Âmbar para mute e alertas. Manrope na interface,
JetBrains Mono em números.

## Product Principles

1. O PA nunca depende do Sona. Retorno para a mesa nasce desligado.
2. Solo e PFL só tocam no fone de quem pediu.
3. Honestidade de estado: o console mostra o que o servidor mede; nada de
   afirmar latência, sincronia ou hardware que não foi medido.
4. O áudio não depende da interface: fechar todas as abas não para o motor.
5. Todo botão diz a consequência antes do clique.

## Evidence on Hand

Testes automáticos e medições em `docs/evidence/`. Sem validação com a Ui24R,
com operadores reais ou na rede da igreja até agora.
